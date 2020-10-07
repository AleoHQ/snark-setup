use crate::{
    environment::Environment,
    objects::{participant::*, Chunk},
    CoordinatorError,
};

use chrono::{DateTime, Utc};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_aux::prelude::*;
use serde_diff::SerdeDiff;
use tracing::{debug, error};

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, SerdeDiff)]
#[serde(rename_all = "camelCase")]
pub struct Round {
    #[serde(deserialize_with = "deserialize_number_from_string")]
    version: u64,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    height: u64,
    #[serde_diff(opaque)]
    started_at: Option<DateTime<Utc>>,
    #[serde_diff(opaque)]
    finished_at: Option<DateTime<Utc>>,
    #[serde(deserialize_with = "deserialize_contributors_from_strings")]
    contributor_ids: Vec<Participant>,
    #[serde(deserialize_with = "deserialize_verifiers_from_strings")]
    verifier_ids: Vec<Participant>,
    chunks: Vec<Chunk>,
}

impl Round {
    /// Creates a new instance of `Round`.
    #[inline]
    pub fn new(
        environment: &Environment,
        round_height: u64,
        started_at: DateTime<Utc>,
        contributor_ids: Vec<Participant>,
        verifier_ids: Vec<Participant>,
    ) -> Result<Self, CoordinatorError> {
        debug!("Creating round {}", round_height);

        // Check that each contributor ID is a contributor participant type.
        {
            let number_of_non_contributors = contributor_ids
                .par_iter()
                .filter(|participant_id| !participant_id.is_contributor())
                .count();
            if number_of_non_contributors > 0 {
                error!(
                    "Found {} participants who are not contributors",
                    number_of_non_contributors
                );
                return Err(CoordinatorError::ExpectedContributor);
            }
        }
        // Check that each verifier ID is a verifier participant type.
        {
            let number_of_non_verifiers = verifier_ids
                .par_iter()
                .filter(|participant_id| !participant_id.is_verifier())
                .count();
            if number_of_non_verifiers > 0 {
                error!("Found {} participants who are not verifiers", number_of_non_verifiers);
                return Err(CoordinatorError::ExpectedVerifier);
            }
        }

        // Fetch the version number and number of chunks from the given environment.
        let version = environment.version();
        let num_chunks = environment.number_of_chunks();

        // Check that the number of chunks is nonzero.
        if num_chunks == 0 {
            return Err(CoordinatorError::NumberOfChunksInvalid);
        }

        // Initialize the chunk verifiers as a list comprising only the coordinator verifier,
        // as this is for initialization.
        let chunk_verifiers = (0..num_chunks)
            .into_par_iter()
            .map(|_| environment.coordinator_verifier())
            .collect::<Vec<_>>();

        // Construct the chunks for this round.
        let chunks: Vec<Chunk> = (0..num_chunks as usize)
            .into_par_iter()
            .zip(chunk_verifiers)
            .map(|(chunk_id, verifier)| {
                Chunk::new(
                    chunk_id as u64,
                    verifier.clone(),
                    environment.contribution_locator(round_height, chunk_id as u64, 0, true),
                )
                .expect("failed to create chunk")
            })
            .collect();

        debug!("Created round {}", round_height);

        Ok(Self {
            version,
            height: round_height,
            started_at: Some(started_at),
            finished_at: None,
            contributor_ids,
            verifier_ids,
            chunks,
        })
    }

    /// Returns the version number set in the round.
    #[inline]
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Returns the height of the round.
    #[inline]
    pub fn round_height(&self) -> u64 {
        self.height
    }

    /// Returns the time the round started at.
    #[inline]
    pub fn started_at(&self) -> &Option<DateTime<Utc>> {
        &self.started_at
    }

    /// Returns the number of contributors authorized for this round.
    #[inline]
    pub fn number_of_contributors(&self) -> u64 {
        self.contributor_ids.len() as u64
    }

    /// Returns a reference to a list of contributors.
    #[inline]
    pub fn get_contributors(&self) -> &Vec<Participant> {
        &self.contributor_ids
    }

    /// Returns a reference to a list of verifiers.
    #[inline]
    pub fn get_verifiers(&self) -> &Vec<Participant> {
        &self.verifier_ids
    }

    /// Returns a reference to the chunk, if it exists.
    /// Otherwise returns `None`.
    #[inline]
    pub fn get_chunk(&self, chunk_id: u64) -> Result<&Chunk, CoordinatorError> {
        // Fetch the chunk with the given chunk ID.
        let chunk = match self.chunks.get(chunk_id as usize) {
            Some(chunk) => chunk,
            _ => return Err(CoordinatorError::ChunkMissing),
        };

        // Check the ID in the chunk matches the given chunk ID.
        match chunk.chunk_id() == chunk_id {
            true => Ok(chunk),
            false => Err(CoordinatorError::ChunkIdMismatch),
        }
    }

    /// Returns a reference to a list of the chunks.
    #[inline]
    pub fn get_chunks(&self) -> &Vec<Chunk> {
        &self.chunks
    }

    /// Returns the expected number of contributions.
    pub fn expected_num_contributions(&self) -> u64 {
        // The expected number of contributions is one more than
        // the total number of authorized contributions to account
        // for the initialization contribution in each round.
        self.number_of_contributors() + 1
    }

    /// Returns `true` if the given participant is authorized as a contributor.
    /// Otherwise returns `false`.
    #[inline]
    pub fn is_authorized_contributor(&self, participant: &Participant) -> bool {
        self.contributor_ids.contains(participant)
    }

    /// Returns `true` if the given participant is authorized as a verifier.
    /// Otherwise returns `false`.
    #[inline]
    pub fn is_authorized_verifier(&self, participant: &Participant) -> bool {
        self.verifier_ids.contains(participant)
    }

    /// Returns `true` if the chunk corresponding to the given chunk ID is
    /// locked by the given participant. Otherwise, returns `false`.
    #[inline]
    pub fn is_chunk_locked_by(&self, chunk_id: u64, participant: &Participant) -> bool {
        match self.get_chunk(chunk_id) {
            Ok(chunk) => chunk.is_locked_by(participant),
            _ => false,
        }
    }

    /// Returns `true` if all contributions in all chunks are verified.
    /// Otherwise, returns `false`.
    #[inline]
    pub fn is_complete(&self) -> bool {
        let expected_num_contributions = self.expected_num_contributions();
        self.chunks
            .par_iter()
            .filter(|chunk| !chunk.is_complete(expected_num_contributions))
            .collect::<Vec<_>>()
            .is_empty()
    }

    ///
    /// Attempts to acquire the lock of a given chunk ID from storage
    /// for a given participant.
    ///
    #[inline]
    pub(crate) fn try_lock_chunk(&mut self, chunk_id: u64, participant: Participant) -> Result<(), CoordinatorError> {
        // If the participant is a contributor ID, check they are authorized to acquire the lock as a contributor.
        if participant.is_contributor() {
            // Check that the contributor is an authorized contributor in this round.
            if !self.is_authorized_contributor(&participant) {
                error!("{} is not an authorized contributor", &participant);
                return Err(CoordinatorError::UnauthorizedChunkContributor);
            }
        }

        // If the participant is a verifier ID, check they are authorized to acquire the lock as a verifier.
        if participant.is_verifier() {
            // Check that the verifier is an authorized verifier in this round.
            if !self.is_authorized_verifier(&participant) {
                error!("{} is not an authorized verifier", &participant);
                return Err(CoordinatorError::UnauthorizedChunkVerifier);
            }
        }

        // Check that the participant does not currently hold a lock to any chunk.
        let number_of_locks_held = self
            .get_chunks()
            .par_iter()
            .filter(|chunk| chunk.is_locked_by(&participant))
            .count();
        if number_of_locks_held > 0 {
            error!("{} already holds the lock on chunk {}", &participant, chunk_id);
            return Err(CoordinatorError::ChunkLockAlreadyAcquired);
        }

        // Attempt to acquire the lock for the given participant ID.
        let expected_num_contributions = self.expected_num_contributions();
        self.get_chunk_mut(chunk_id)?
            .acquire_lock(participant.clone(), expected_num_contributions)?;

        debug!("{} acquired lock on chunk {}", participant, chunk_id);
        Ok(())
    }

    ///
    /// Updates the contribution corresponding to a given chunk ID and
    /// contribution ID as verified.
    ///
    /// This function assumes the current contribution already has
    /// a verifier assigned to it.
    ///
    #[inline]
    pub(crate) fn verify_contribution(
        &mut self,
        chunk_id: u64,
        contribution_id: u64,
        participant: Participant,
        verified_locator: String,
    ) -> Result<(), CoordinatorError> {
        // Set the current contribution as verified for the given chunk ID.
        self.get_chunk_mut(chunk_id)?
            .verify_contribution(contribution_id, participant, verified_locator)?;

        // If the chunk is complete and the finished at timestamp has not been set yet,
        // then set it with the current UTC timestamp.
        if self.is_complete() && self.finished_at.is_none() {
            self.finished_at = Some(Utc::now());
        }

        Ok(())
    }

    /// Returns a mutable reference to the chunk, if it exists.
    /// Otherwise returns `None`.
    #[inline]
    pub(crate) fn get_chunk_mut(&mut self, chunk_id: u64) -> Result<&mut Chunk, CoordinatorError> {
        // Fetch the chunk with the given chunk ID.
        let chunk = match self.chunks.get_mut(chunk_id as usize) {
            Some(chunk) => chunk,
            _ => return Err(CoordinatorError::ChunkMissing),
        };

        // Check the ID in the chunk matches the given chunk ID.
        match chunk.chunk_id() == chunk_id {
            true => Ok(chunk),
            false => Err(CoordinatorError::ChunkIdMismatch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::prelude::*;

    #[test]
    #[serial]
    fn test_round_0_matches() {
        let expected = test_round_0().unwrap();
        let candidate = Round::new(
            &TEST_ENVIRONMENT,
            0, /* height */
            *TEST_STARTED_AT,
            vec![],
            TEST_VERIFIER_IDS.to_vec(),
        )
        .unwrap();

        if candidate != expected {
            print_diff(&expected, &candidate);
        }
        assert_eq!(candidate, expected);
    }

    #[test]
    #[serial]
    fn test_round_height() {
        let round_0 = test_round_0_json().unwrap();
        assert_eq!(0, round_0.round_height());

        let round_0 = test_round_0().unwrap();
        assert_eq!(0, round_0.round_height());

        let round_1 = test_round_1_initial_json().unwrap();
        assert_eq!(1, round_1.round_height());
    }

    #[test]
    #[serial]
    fn test_is_authorized_contributor() {
        let round_1 = test_round_1_initial_json().unwrap();
        assert!(round_1.is_authorized_contributor(&TEST_CONTRIBUTOR_ID));
    }

    #[test]
    #[serial]
    fn test_is_authorized_verifier() {
        let round_0 = test_round_0().unwrap();
        assert!(round_0.is_authorized_verifier(&TEST_VERIFIER_ID));

        let round_1 = test_round_1_initial_json().unwrap();
        assert!(round_1.is_authorized_contributor(&TEST_CONTRIBUTOR_ID));
    }

    #[test]
    #[serial]
    fn test_get_chunk() {
        let expected = test_round_0_json().unwrap().chunks[0].clone();
        let candidate = test_round_0().unwrap().get_chunk(0).unwrap().clone();
        print_diff(&expected, &candidate);
        assert_eq!(expected, candidate);
    }

    #[test]
    #[serial]
    fn test_get_chunk_mut_basic() {
        let expected = test_round_0_json().unwrap().chunks[0].clone();
        let candidate = test_round_0().unwrap().get_chunk_mut(0).unwrap().clone();
        print_diff(&expected, &candidate);
        assert_eq!(expected, candidate);
    }

    #[test]
    #[ignore]
    #[serial]
    fn test_get_chunk_mut() {
        let mut expected = test_round_1_initial_json().unwrap().chunks[0].clone();
        expected
            .acquire_lock(Participant::Contributor("test-coordinator-contributor".to_string()), 2)
            .unwrap();

        let mut candidate = test_round_0().unwrap().get_chunk_mut(0).unwrap().clone();
        candidate
            .acquire_lock(Participant::Contributor("test-coordinator-contributor".to_string()), 2)
            .unwrap();

        print_diff(&expected, &candidate);
        assert_eq!(expected, candidate);
    }

    #[test]
    #[serial]
    fn test_get_verifiers() {
        let candidates = test_round_0().unwrap().get_verifiers().clone();
        for (index, id) in TEST_VERIFIER_IDS.iter().enumerate() {
            assert_eq!(*id, candidates[index]);
        }
    }

    #[test]
    #[serial]
    fn test_is_complete() {
        // TODO (howardwu): Add tests for a full completeness check.
        let round_0 = test_round_0_json().unwrap();
        assert!(round_0.is_complete());

        let round_0 = test_round_0().unwrap();
        assert!(round_0.is_complete());

        let round_1 = test_round_1_initial_json().unwrap();
        assert!(!round_1.is_complete());
    }
}
