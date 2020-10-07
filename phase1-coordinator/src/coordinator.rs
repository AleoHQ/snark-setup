use crate::{
    commands::{Aggregation, Computation, Initialization, Verification},
    environment::Environment,
    objects::{Participant, Round},
    storage::{Key, Storage, Value},
};

use chrono::{DateTime, Utc};
use std::{
    collections::HashSet,
    fmt,
    sync::{Arc, RwLock, RwLockReadGuard},
};
use tracing::{debug, error, info, trace};

#[derive(Debug)]
pub enum CoordinatorError {
    ChunkAlreadyComplete,
    ChunkAlreadyVerified,
    ChunkIdMismatch,
    ChunkLockAlreadyAcquired,
    ChunkMissing,
    ChunkMissingTranscript,
    ChunkMissingVerification,
    ChunkNotLocked,
    ChunkNotLockedOrByWrongParticipant,
    ChunkUpdateFailed,
    ChunkVerifierMissing,
    ComputationFailed,
    ContributionAlreadyAssignedVerifiedLocator,
    ContributionAlreadyAssignedVerifier,
    ContributionAlreadyVerified,
    ContributionFileSizeMismatch,
    ContributionIdIsNonzero,
    ContributionIdMismatch,
    ContributionLocatorAlreadyExists,
    ContributionLocatorMissing,
    ContributionMissing,
    ContributionMissingVerification,
    ContributionMissingVerifiedLocator,
    ContributionMissingVerifier,
    ContributionShouldNotExist,
    ContributionVerificationFailed,
    ContributionsComplete,
    ContributorAlreadyContributed,
    ContributorsMissing,
    ExpectedContributor,
    ExpectedVerifier,
    Error(anyhow::Error),
    InitializationFailed,
    InitializationTranscriptsDiffer,
    InvalidUrl,
    IOError(std::io::Error),
    JsonError(serde_json::Error),
    Launch(rocket::error::LaunchError),
    MissingVerifierIds,
    NumberOfChunksInvalid,
    NumberOfChunkVerifierIdsInvalid,
    NumberOfChunkVerifiedBaseUrlsInvalid,
    NumberOfContributionsDiffer,
    RoundAggregationFailed,
    RoundAlreadyInitialized,
    RoundChunksMissingVerification,
    RoundDirectoryMissing,
    RoundDoesNotExist,
    RoundHeightIsZero,
    RoundHeightMismatch,
    RoundLocatorAlreadyExists,
    RoundLocatorMissing,
    RoundNotComplete,
    RoundNotVerified,
    RoundSkipped,
    StorageFailed,
    StorageReaderFailed,
    StorageUpdateFailed,
    StorageWriterFailed,
    UnauthorizedChunkContributor,
    UnauthorizedChunkVerifier,
    Url(url::ParseError),
    VerificationFailed,
    VerificationOnContributionIdZero,
    VerifierMissing,
}

impl From<anyhow::Error> for CoordinatorError {
    fn from(error: anyhow::Error) -> Self {
        CoordinatorError::Error(error)
    }
}

impl From<serde_json::Error> for CoordinatorError {
    fn from(error: serde_json::Error) -> Self {
        CoordinatorError::JsonError(error)
    }
}

impl From<std::io::Error> for CoordinatorError {
    fn from(error: std::io::Error) -> Self {
        CoordinatorError::IOError(error)
    }
}

impl From<url::ParseError> for CoordinatorError {
    fn from(error: url::ParseError) -> Self {
        CoordinatorError::Url(error)
    }
}

impl fmt::Display for CoordinatorError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<CoordinatorError> for anyhow::Error {
    fn from(error: CoordinatorError) -> Self {
        Self::msg(error.to_string())
    }
}

/// A core structure for operating the Phase 1 ceremony.
#[derive(Clone)]
pub struct Coordinator {
    storage: Arc<RwLock<Box<dyn Storage>>>,
    environment: Environment,
}

impl Coordinator {
    ///
    /// Creates a new instance of the `Coordinator`, for a given environment.
    ///
    /// The coordinator loads and instantiates an internal instance of storage.
    /// All subsequent interactions with the coordinator are directly from storage.
    ///
    /// The coordinator is forbidden from caching state about any round.
    ///
    #[inline]
    pub fn new(environment: Environment) -> Result<Self, CoordinatorError> {
        Ok(Self {
            storage: Arc::new(RwLock::new(environment.storage()?)),
            environment,
        })
    }

    ///
    /// Returns a reference to the instantiation of `Environment` that this
    /// coordinator is using.
    ///
    #[inline]
    pub fn environment(&self) -> &Environment {
        &self.environment
    }

    ///
    /// Returns `true` if the given participant is a contributor and included
    /// in the list of contributors for the current round of the ceremony.
    ///
    /// If the participant is not a contributor, or if there are
    /// no prior rounds, returns `false`.
    ///
    #[inline]
    pub fn is_current_contributor(&self, participant: &Participant) -> bool {
        // Check that the participant is a contributor.
        if !participant.is_contributor() {
            return false;
        }
        // Check that the participant is not a verifier.
        if participant.is_verifier() {
            return false;
        }
        // Check that the participant is a contributor in the current round.
        match self.current_round() {
            Ok(round) => round.is_authorized_contributor(participant),
            _ => false,
        }
    }

    ///
    /// Returns `true` if the given participant is a verifier and included
    /// in the list of verifiers for the current round of the ceremony.
    ///
    /// If the participant is not a verifier, or if there are
    /// no prior rounds, returns a `false`.
    ///
    #[inline]
    pub fn is_current_verifier(&self, participant: &Participant) -> bool {
        // Check that the participant is not a contributor.
        if participant.is_contributor() {
            return false;
        }
        // Check that the participant is a verifier.
        if !participant.is_verifier() {
            return false;
        }
        // Check that the participant is a verifier in the current round.
        match self.current_round() {
            Ok(round) => round.is_authorized_verifier(participant),
            _ => false,
        }
    }

    ///
    /// Returns a reference to the round corresponding to the given height from storage.
    ///
    /// If there are no prior rounds, returns a `CoordinatorError`.
    ///
    #[inline]
    pub fn get_round(&self, round_height: u64) -> Result<Round, CoordinatorError> {
        // Load the round corresponding to the given round height from storage.
        match self.storage()?.get(&Key::Round(round_height)) {
            Some(Value::Round(round)) => Ok(round.clone()),
            _ => Err(CoordinatorError::RoundDoesNotExist),
        }
    }

    ///
    /// Returns a reference to the current round of the ceremony
    /// from storage, irrespective of the stage of its completion.
    ///
    /// If there are no prior rounds in storage, returns `0`.
    ///
    /// When loading the current round from storage, this function
    /// checks that the current round height matches the height
    /// set in the returned `Round` instance.
    ///
    #[inline]
    pub fn current_round(&self) -> Result<Round, CoordinatorError> {
        // Fetch the current round height.
        let round_height = self.current_round_height()?;
        // Fetch the corresponding round.
        let round = self.get_round(round_height)?;
        // Check that the round height matches the current round height.
        match round.round_height() == round_height {
            true => Ok(round),
            false => Err(CoordinatorError::RoundHeightMismatch),
        }
    }

    ///
    /// Returns the current round height of the ceremony from storage,
    /// irrespective of the stage of its completion.
    ///
    /// For convention, a round height of `0` indicates that there have
    /// been no prior rounds of the ceremony. The ceremony is initialized
    /// on a round height of `0` and the first round of public contribution
    /// starts on a round height of `1`.
    ///
    /// When loading the current round height from storage, this function
    /// checks that the corresponding round is in storage. Note that it
    /// only checks for the existence of a round value and does not
    /// check for its correctness.
    ///
    #[inline]
    pub fn current_round_height(&self) -> Result<u64, CoordinatorError> {
        // Acquire the storage read lock.
        let storage = self.storage()?;

        // Fetch the latest round height from storage.
        match storage.get(&Key::RoundHeight) {
            Some(Value::RoundHeight(round_height)) => match round_height != 0 {
                // Case 1 - This is a typical round of the ceremony.
                // Check that the corresponding round data exists in storage.
                true => match storage.contains_key(&Key::Round(round_height)) {
                    true => Ok(round_height),
                    false => Err(CoordinatorError::StorageFailed),
                },
                // Case 2 - There are no prior rounds of the ceremony.
                false => Ok(0),
            },
            // Case 2 - There are no prior rounds of the ceremony.
            _ => Ok(0),
        }
    }

    ///
    /// Returns the current contribution locator for a given chunk ID.
    ///
    /// If the current contribution is NOT contributed yet,
    /// this function will return a `CoordinatorError`.
    ///
    /// If the current contribution is already verified,
    /// this function will return a `CoordinatorError`.
    ///
    #[inline]
    pub fn current_contribution_locator(&self, chunk_id: u64, verified: bool) -> Result<String, CoordinatorError> {
        // Fetch the current round.
        let current_round = self.current_round()?;
        // Fetch the current round height.
        let current_round_height = current_round.round_height();
        // Fetch the chunk corresponding to the given chunk ID.
        let chunk = current_round.get_chunk(chunk_id)?;
        // Fetch the current contribution ID.
        let current_contribution_id = chunk.current_contribution_id();

        // Check that the contribution locator corresponding to the current contribution ID
        // exists for the current round and given chunk ID.
        if !self.environment.contribution_locator_exists(
            current_round_height,
            chunk_id,
            current_contribution_id,
            verified,
        ) {
            return Err(CoordinatorError::ContributionLocatorMissing);
        }

        // Check that the current contribution ID is NOT verified yet.
        if chunk.get_contribution(current_contribution_id)?.is_verified() {
            return Err(CoordinatorError::ContributionAlreadyVerified);
        }

        // Fetch the current contribution locator.
        let current_contribution_locator =
            self.environment
                .contribution_locator(current_round_height, chunk_id, current_contribution_id, verified);

        Ok(current_contribution_locator)
    }

    ///
    /// Returns the next contribution locator for a given chunk ID.
    ///
    /// If the current contribution is NOT contributed yet,
    /// this function will return a `CoordinatorError`.
    ///
    /// If the current contribution is NOT verified yet,
    /// this function will return a `CoordinatorError`.
    ///
    /// If the next contribution locator already exists,
    /// this function will return a `CoordinatorError`.
    ///
    /// If the chunk corresponding to the given chunk ID
    /// is already completed for the current round,
    /// this function will return a `CoordinatorError`.
    ///
    #[inline]
    pub fn next_contribution_locator(&self, chunk_id: u64) -> Result<String, CoordinatorError> {
        // Fetch the current round.
        let current_round = self.current_round()?;
        // Fetch the current round height.
        let current_round_height = current_round.round_height();
        // Fetch the expected number of contributions for the current round.
        let expected_num_contributions = current_round.expected_num_contributions();
        // Fetch the chunk corresponding to the given chunk ID.
        let chunk = current_round.get_chunk(chunk_id)?;
        // Fetch the next contribution ID.
        let next_contribution_id = chunk.next_contribution_id(expected_num_contributions)?;

        // Check that the current contribution has been verified.
        if !chunk.current_contribution()?.is_verified() {
            return Err(CoordinatorError::ContributionMissingVerification);
        }

        // Check that the contribution locator corresponding to the next contribution ID
        // does NOT exist for the current round and given chunk ID.
        if self
            .environment
            .contribution_locator_exists(current_round_height, chunk_id, next_contribution_id, false)
        {
            return Err(CoordinatorError::ContributionLocatorAlreadyExists);
        }

        // Fetch the next contribution locator.
        let next_contribution_locator =
            self.environment
                .contribution_locator(current_round_height, chunk_id, next_contribution_id, false);

        Ok(next_contribution_locator)
    }

    ///
    /// Attempts to acquire the lock of a given chunk ID from storage
    /// for a given participant.
    ///
    /// On success, this function returns the next contribution locator
    /// if the participant is a contributor, and it returns the current
    /// contribution locator if the participant is a verifier.
    ///
    /// On failure, this function returns a `CoordinatorError`.
    ///
    #[inline]
    pub fn try_lock_chunk(&self, chunk_id: u64, participant: Participant) -> Result<String, CoordinatorError> {
        // Fetch the current round.
        let current_round = self.current_round()?;
        // Fetch the current round height.
        let current_round_height = current_round.round_height();
        // Fetch the chunk corresponding to the given chunk ID.
        let chunk = current_round.get_chunk(chunk_id)?;
        // Fetch the next contribution ID.
        let current_contribution = chunk.current_contribution()?;

        // Check that the participant is authorized to acquire the lock
        // associated with the given chunk ID for the current round,
        // and fetch the appropriate contribution locator.
        let contribution_locator = match participant.clone() {
            Participant::Contributor(_) => {
                // Check that the participant is an authorized contributor
                // for the current round.
                if !self.is_current_contributor(&participant) {
                    return Err(CoordinatorError::UnauthorizedChunkContributor);
                }
                // This call enforces a strict check that the
                // next contribution locator does NOT exist and
                // that the current contribution locator exists
                // and has already been verified.
                self.next_contribution_locator(chunk_id)?
            }
            Participant::Verifier(_) => {
                // Check that the participant is an authorized verifier
                // for the current round.
                if !self.is_current_verifier(&participant) {
                    return Err(CoordinatorError::UnauthorizedChunkVerifier);
                }
                // This call enforces a strict check that the
                // current contribution locator exist and
                // has not been verified yet.
                self.current_contribution_locator(chunk_id, false)?
            }
        };

        // As a corollary, if the current contribution locator exists
        // and the current contribution has not been verified yet,
        // check that the given participant is not a contributor.
        if self.current_contribution_locator(chunk_id, false).is_ok() && !current_contribution.is_verified() {
            // Check that the given participant is not a contributor.
            if participant.is_contributor() {
                return Err(CoordinatorError::UnauthorizedChunkContributor);
            }
        }

        // Attempt to acquire the chunk lock for verification.
        {
            // Create a mutable reference of the current round from storage.
            let mut current_round = current_round.clone();

            // Attempt to acquire the chunk lock for participant.
            current_round.try_lock_chunk(chunk_id, participant.clone())?;

            // Save the update to storage.
            self.save_round_to_storage(current_round_height, current_round)?;
        }

        info!(
            "{} acquired lock on round {} chunk {}",
            participant, current_round_height, chunk_id
        );

        Ok(contribution_locator)
    }

    ///
    /// Attempts to add a contribution for a given chunk ID from a given participant.
    ///
    /// On success, this function releases the lock from the contributor and returns
    /// the chunk locator.
    ///
    /// On failure, it returns a `CoordinatorError`.
    ///
    #[inline]
    pub fn add_contribution(&self, chunk_id: u64, participant: Participant) -> Result<String, CoordinatorError> {
        info!("Attempting to add a contribution to chunk {}", chunk_id);

        // Check that the participant is a contributor.
        if !participant.is_contributor() {
            return Err(CoordinatorError::UnauthorizedChunkContributor);
        }

        // Check that the participant is an authorized contributor to the round.
        if !self.is_current_contributor(&participant) {
            error!("{} is unauthorized to contribute to chunk {})", &participant, chunk_id);
            return Err(CoordinatorError::UnauthorizedChunkContributor);
        }

        // Check that the ceremony started and exists in storage.
        let current_round_height = self.current_round_height()?;
        if current_round_height == 0 {
            error!("The ceremony has not started");
            return Err(CoordinatorError::RoundHeightIsZero);
        }

        // Check that the chunk lock is currently held by this contributor.
        let current_round = self.get_round(current_round_height)?;
        if !current_round.is_chunk_locked_by(chunk_id, &participant) {
            error!("{} should have lock on chunk {} but does not", &participant, chunk_id);
            return Err(CoordinatorError::ChunkNotLockedOrByWrongParticipant);
        }

        // Fetch the next contribution ID of the chunk.
        let expected_num_contributions = current_round.expected_num_contributions();
        let next_contribution_id = self
            .current_round()?
            .get_chunk(chunk_id)?
            .next_contribution_id(expected_num_contributions)?;

        // Fetch the contribution locator for the next contribution ID corresponding to
        // the current round height and chunk ID.
        let next_contributed_locator = self.next_contribution_locator_unchecked(chunk_id)?;
        trace!("Next contribution locator is {}", next_contributed_locator);

        {
            // TODO (howardwu): Check that the file size is nonzero, the structure is correct,
            //  and the starting hash is based on the previous contribution.
        }

        // Add the next contribution to the current chunk.
        {
            // Create a mutable reference of the current round from storage.
            let mut current_round = current_round.clone();

            // Add the next contribution to the current chunk.
            current_round.get_chunk_mut(chunk_id)?.add_contribution(
                next_contribution_id,
                participant,
                next_contributed_locator.clone(),
                expected_num_contributions,
            )?;

            // Save the update to storage.
            self.save_round_to_storage(current_round_height, current_round)?;
        }

        {
            // TODO (howardwu): Send job to run verification on new chunk.
            // TODO (kobigurk): if verification fails, reset round
        }

        Ok(next_contributed_locator)
    }

    ///
    /// Attempts to run verification in the current round for a given chunk ID and participant.
    ///
    /// On success, this function copies the current contribution into the next transcript locator,
    /// which is the next contribution ID within a round, or the next round height if this round
    /// is complete.
    ///
    #[inline]
    pub fn verify_contribution(&self, chunk_id: u64, participant: Participant) -> Result<(), CoordinatorError> {
        // Check that the given participant is a verifier.
        if !participant.is_verifier() {
            return Err(CoordinatorError::ExpectedVerifier);
        }

        // Fetch the current round.
        let current_round = self.current_round()?;

        // Check that the chunk lock is currently held by this verifier.
        if !current_round.is_chunk_locked_by(chunk_id, &participant) {
            error!("{} should have lock on chunk {} but does not", &participant, chunk_id);
            return Err(CoordinatorError::ChunkNotLockedOrByWrongParticipant);
        }

        // Fetch the current round height.
        let current_round_height = current_round.round_height();
        // Fetch the chunk corresponding to the given chunk ID.
        let chunk = current_round.get_chunk(chunk_id)?;
        // Fetch the current contribution ID.
        let current_contribution_id = chunk.current_contribution_id();
        // Fetch the next contribution ID.
        let current_contribution = chunk.current_contribution()?;

        // Check that the contribution locator corresponding to this round and chunk exists.
        if !self
            .environment
            .contribution_locator_exists(current_round_height, chunk_id, current_contribution_id, false)
        {
            return Err(CoordinatorError::ContributionLocatorMissing);
        }

        // Check if the current contribution has already been verified.
        if current_contribution.is_verified() {
            return Err(CoordinatorError::ContributionAlreadyVerified);
        }

        // Fetch the contribution locators for `Verification`.
        let (previous, current, next) = if chunk.only_contributions_complete(current_round.expected_num_contributions())
        {
            let previous = self.environment.contribution_locator(
                current_round_height,
                chunk_id,
                current_contribution_id - 1,
                true,
            );
            let current =
                self.environment
                    .contribution_locator(current_round_height, chunk_id, current_contribution_id, false);
            let next = self
                .environment
                .contribution_locator(current_round_height + 1, chunk_id, 0, true);

            // Initialize the chunk directory of the new round so the next locator file will be saved.
            self.environment
                .chunk_directory_init(current_round_height + 1, chunk_id);

            (previous, current, next)
        } else {
            let previous = self.environment.contribution_locator(
                current_round_height,
                chunk_id,
                current_contribution_id - 1,
                true,
            );
            let current =
                self.environment
                    .contribution_locator(current_round_height, chunk_id, current_contribution_id, false);
            let next =
                self.environment
                    .contribution_locator(current_round_height, chunk_id, current_contribution_id, true);
            (previous, current, next)
        };

        debug!("Coordinator is starting verification on chunk {}", chunk_id);
        Verification::run(
            &self.environment,
            current_round_height,
            chunk_id,
            current_contribution_id,
            previous,
            current.clone(),
            next,
        )?;
        debug!("Coordinator completed verification on chunk {}", chunk_id);

        // Attempts to set the current contribution as verified in the current round.
        {
            // Create a mutable reference of the current round from storage.
            let mut current_round = current_round.clone();

            // Attempts to set the current contribution as verified in the current round.
            current_round.verify_contribution(chunk_id, current_contribution_id, participant.clone(), current)?;

            // Save the update to storage.
            self.save_round_to_storage(current_round_height, current_round)?;
        }

        info!(
            "{} verified chunk {} contribution {}",
            participant, chunk_id, current_contribution_id
        );
        Ok(())
    }

    #[inline]
    pub fn reset_round(
        &self,
        started_at: DateTime<Utc>,
        backup_tag: &str,
        contributors: Vec<Participant>,
        verifiers: Vec<Participant>,
    ) -> Result<u64, CoordinatorError> {
        // Check that the round has at least one authorized contributor.
        if contributors.is_empty() {
            return Err(CoordinatorError::ContributorsMissing);
        }

        // Check that the round has at least one authorized verifier.
        if verifiers.is_empty() {
            return Err(CoordinatorError::VerifierMissing);
        }

        // Fetch the current height of the ceremony.
        let round_height = self.current_round_height()?;
        info!("Restarting current round {}", round_height);

        let current_round = self.get_round(round_height)?;
        {
            let mut storage = match self.storage.write() {
                Ok(storage) => storage,
                _ => return Err(CoordinatorError::StorageWriterFailed),
            };
            if !storage.save_backup(backup_tag) {
                error!(
                    "Could not save storage backup for round {} with tag {}",
                    round_height, backup_tag
                );
                return Err(CoordinatorError::StorageUpdateFailed);
            }
            if !storage.remove(&Key::Round(round_height)) {
                error!("Could not remove round {} from storage", round_height);
                return Err(CoordinatorError::StorageUpdateFailed);
            }
            if !storage.remove(&Key::RoundHeight) {
                error!("Could not remove round height from storage");
                return Err(CoordinatorError::StorageUpdateFailed);
            }
            if !storage.insert(Key::RoundHeight, Value::RoundHeight(round_height - 1)) {
                error!("Could not insert round height to storage");
                return Err(CoordinatorError::StorageUpdateFailed);
            }
        }

        // If the round is complete, we also need to clear the next round directory.
        // No need to back it up since it's derived from the backed up round.
        if current_round.is_complete() {
            self.environment.round_directory_reset(round_height + 1);
        }

        self.environment
            .round_directory_reset_and_backup(round_height, backup_tag);

        // Execute the round initialization as the coordinator.
        // On success, the reset round will have been saved to storage.
        self.run_initialization(round_height - 1, started_at, contributors, verifiers)?;

        // Fetch the new round height.
        let new_height = self.current_round_height()?;

        // Check that the new height is the same.
        if new_height != round_height {
            error!("Round height after initialization is {}", new_height);
            return Err(CoordinatorError::RoundHeightMismatch);
        }

        info!("Completed restarting round {}", round_height);
        Ok(new_height)
    }

    ///
    /// Initiates the next round of the ceremony.
    ///
    /// If there are no prior rounds in storage, this initializes a new ceremony
    /// by invoking `Initialization`, and saves it to storage.
    ///
    /// Otherwise, this loads the current round from storage and checks that
    /// it is fully verified before proceeding to aggregate the round, and
    /// initialize the next round, saving it to storage for the coordinator.
    ///
    /// In a test environment, this function resets the transcript for the
    /// coordinator when round height is 0.
    /// In a development or production environment, this does NOT reset the
    /// transcript for the coordinator.
    ///
    /// On success, the function returns the new round height.
    /// Otherwise, it returns a `CoordinatorError`.
    ///
    #[inline]
    pub fn next_round(
        &self,
        started_at: DateTime<Utc>,
        contributors: Vec<Participant>,
        verifiers: Vec<Participant>,
    ) -> Result<u64, CoordinatorError> {
        // Check that the next round has at least one authorized contributor.
        if contributors.is_empty() {
            return Err(CoordinatorError::ContributorsMissing);
        }

        // Check that the next round has at least one authorized verifier.
        if verifiers.is_empty() {
            return Err(CoordinatorError::VerifierMissing);
        }

        // Fetch the current height of the ceremony.
        let round_height = self.current_round_height()?;
        trace!("Current round height from storage is {}", round_height);

        let next_height = round_height + 1;
        info!("Starting transition from round {} to {}", round_height, next_height);

        // Execute aggregation of the current round in preparation for
        // transition to next round. If this is the initial round, there
        // should be nothing to aggregate and we may continue.
        if round_height != 0 {
            // Attempt to fetch the current round directly.
            let current_round = self.get_round(round_height);
            trace!("Check current round exists in storage ({})", current_round.is_ok());

            // Check that all chunks in the current round are verified,
            // so that we may transition to the next round.
            let current_round = current_round?;
            if !&current_round.is_complete() {
                error!("Round {} is not complete and next round is not starting", round_height);
                trace!("{:#?}", &current_round);
                return Err(CoordinatorError::RoundNotComplete);
            }

            // Execute round aggregation and aggregate verification on the current round.
            if let Err(_) = self.run_aggregation() {
                self.reset_round(
                    Utc::now(),
                    &Utc::now().timestamp_nanos().to_string(),
                    contributors,
                    verifiers,
                )?;
                error!(
                    "Round {} aggregation failed. Reset round and continue normally.",
                    round_height
                );
                return Ok(round_height);
            };
        }

        // Execute the round initialization as the coordinator.
        // On success, the new round will have been saved to storage.
        self.run_initialization(round_height, started_at, contributors, verifiers)?;

        // Fetch the new round height.
        let new_height = self.current_round_height()?;

        // Check that the new height increments the prior round height by 1.
        if new_height != next_height {
            error!("Round height after initialization is {}", new_height);
            return Err(CoordinatorError::RoundHeightMismatch);
        }

        info!("Completed transition from round {} to {}", round_height, new_height);
        Ok(new_height)
    }

    // /// Attempts to run verification in the current round for a given chunk ID.
    // #[inline]
    // fn verify_chunk(&self, chunk_id: u64) -> Result<(), CoordinatorError> {
    //     // Fetch the current round.
    //     let mut current_round = self.current_round()?;
    //     let round_height = current_round.round_height();
    //
    //     // Execute verification of contribution ID for all chunks in the
    //     // new round and check that the new locators exist.
    //     let new_height = round_height + 1;
    //     debug!("Starting verification of round {}", new_height);
    //     for chunk_id in 0..self.environment.number_of_chunks() {
    //     info!("Coordinator is starting initialization on chunk {}", chunk_id);
    //     // TODO (howardwu): Add contribution hash to `Round`.
    //     let _contribution_hash = Initialization::run(&self.environment, new_height, chunk_id)?;
    //     info!("Coordinator completed initialization on chunk {}", chunk_id);
    //
    //     // Check that the contribution locator corresponding to this round and chunk now exists.
    //     let contribution_locator = self.environment.contribution_locator(new_height, chunk_id, contribution_id);
    //     if !Path::new(&contribution_locator).exists() {
    //         return Err(CoordinatorError::RoundTranscriptMissing);
    //     }
    //
    //     // Attempt to acquire the lock for verification.
    //     // self.try_lock(chunk_id, contribution_id)?;
    //
    //     // Runs verification and on success, updates the chunk contribution to verified.
    //     // self.verify_contribution(chunk_id, contribution_id)?;
    // }

    ///
    /// Returns the next contribution locator for a given chunk ID.
    ///
    /// If the chunk corresponding to the given chunk ID
    /// is already completed for the current round,
    /// this function will return a `CoordinatorError`.
    ///
    #[inline]
    fn next_contribution_locator_unchecked(&self, chunk_id: u64) -> Result<String, CoordinatorError> {
        // Fetch the current round.
        let current_round = self.current_round()?;
        // Fetch the current round height.
        let current_round_height = current_round.round_height();
        // Fetch the expected number of contributions for the current round.
        let expected_num_contributions = current_round.expected_num_contributions();
        // Fetch the chunk corresponding to the given chunk ID.
        let chunk = current_round.get_chunk(chunk_id)?;
        // Fetch the next contribution ID.
        let next_contribution_id = chunk.next_contribution_id(expected_num_contributions)?;
        // Fetch the next contribution locator.
        let next_contribution_locator =
            self.environment
                .contribution_locator(current_round_height, chunk_id, next_contribution_id, false);

        Ok(next_contribution_locator)
    }

    ///
    /// Attempts to run initialization for a given round.
    ///
    /// In a test environment, this function clears the transcript for the
    /// coordinator. In a development or production environment, this
    /// does NOT reset the transcript for the coordinator.
    ///
    #[inline]
    fn run_initialization(
        &self,
        round_height: u64,
        started_at: DateTime<Utc>,
        contributors: Vec<Participant>,
        verifiers: Vec<Participant>,
    ) -> Result<(), CoordinatorError> {
        trace!("Received call to initialize round {}", round_height);

        // Fetch the current round height.
        let current_round_height = self.current_round_height()?;

        // Check that the given round height is above the current round height.
        if round_height < current_round_height {
            error!("Round {} is less than round {}", round_height, current_round_height);
            return Err(CoordinatorError::RoundAlreadyInitialized);
        }
        // Check that the given round height corresponds to the current round height.
        if round_height != current_round_height {
            error!("Expected round height {} == {}", round_height, current_round_height);
            return Err(CoordinatorError::RoundHeightMismatch);
        }

        // If this is the initial round, ensure the round does not exist yet.
        // Attempt to load the round corresponding to the given round height from storage.
        // If there is no round in storage, proceed to create a new round instance,
        // and run `Initialization` to start the ceremony.
        if round_height == 0 {
            // If the path exists, this means a prior *ceremony* is stored as a transcript.
            if self.environment.round_directory_exists(round_height) {
                self.environment.round_directory_reset(round_height);
            }

            // Create an instantiation of `Round` for round 0.
            let round = {
                // Initialize the contributors as an empty list as this is for initialization.
                let contributors = vec![];

                // Initialize the verifiers as a list comprising only the coordinator verifier,
                // as this is for initialization.
                let verifiers = vec![self.environment.coordinator_verifier()];

                match self.storage()?.get(&Key::Round(round_height)) {
                    // Check that the round does not exist in storage.
                    // If it exists, this means the round was already initialized.
                    Some(Value::Round(_)) => {
                        // This is OK, because a round might have been reset, so just return an instance.
                        Round::new(&self.environment, round_height, started_at, contributors, verifiers)?
                    }
                    Some(_) => return Err(CoordinatorError::StorageFailed),
                    // Create a new round instance and save it to storage.
                    _ => Round::new(&self.environment, round_height, started_at, contributors, verifiers)?,
                }
            };

            debug!("Starting initialization of round {}", round_height);

            // Execute initialization of contribution 0 for all chunks in the
            // new round and check that the new locators exist.
            for chunk_id in 0..self.environment.number_of_chunks() {
                info!("Coordinator is starting initialization on chunk {}", chunk_id);
                // TODO (howardwu): Add contribution hash to `Round`.
                let _contribution_hash = Initialization::run(&self.environment, round_height, chunk_id)?;
                info!("Coordinator completed initialization on chunk {}", chunk_id);

                // 1 - Check that the contribution locator corresponding to this round's chunk now exists.
                if !self
                    .environment
                    .contribution_locator_exists(round_height, chunk_id, 0, true)
                {
                    return Err(CoordinatorError::ContributionLocatorMissing);
                }

                // 2 - Check that the contribution locator corresponding to the next round's chunk now exists.
                if !self
                    .environment
                    .contribution_locator_exists(round_height + 1, chunk_id, 0, true)
                {
                    return Err(CoordinatorError::ContributionLocatorMissing);
                }
            }

            // Write the round to storage.
            self.save_round_to_storage(round_height, round)?;

            // Check that the current round now matches the given round height unconditionally.
            if self.current_round_height()? != round_height {
                return Err(CoordinatorError::RoundHeightMismatch);
            }

            debug!("Completed initialization of round {}", round_height);
        }

        // Create the new round height.
        let new_height = round_height + 1;

        debug!("Starting initialization of round {}", new_height);

        // Check that the new round does not exist in storage.
        // If it exists, this means the round was already initialized.
        match self.storage()?.get(&Key::Round(new_height)) {
            Some(Value::Round(_)) => return Err(CoordinatorError::RoundAlreadyInitialized),
            Some(_) => {
                return Err(CoordinatorError::StorageFailed);
            }
            _ => (),
        };

        // Check that each contribution transcript for the next round exists.
        for chunk_id in 0..self.environment.number_of_chunks() {
            debug!("Locating round {} chunk {} contribution 0", new_height, chunk_id);
            if !self
                .environment
                .contribution_locator_exists(new_height, chunk_id, 0, true)
            {
                return Err(CoordinatorError::ContributionLocatorMissing);
            }
        }

        // Instantiate the new round and height.
        let new_round = Round::new(&self.environment, new_height, started_at, contributors, verifiers)?;

        #[cfg(test)]
        {
            trace!("{:?}", &new_round);
        }

        // Insert and save the new round into storage.
        self.save_round_to_storage(new_height, new_round)?;

        debug!("Completed initialization of round {}", new_height);
        Ok(())
    }

    ///
    /// Attempts to run computation for a given chunk ID and contribution ID in the current round.
    ///
    /// This function is primarily used for testing purposes. This can also be purposed for
    /// completing contributions of participants who may have dropped off and handed over
    /// control of their session.
    ///
    #[inline]
    #[allow(dead_code)]
    fn run_computation(
        &self,
        chunk_id: u64,
        contribution_id: u64,
        participant: &Participant,
    ) -> Result<(), CoordinatorError> {
        // Fetch the current height from storage.
        let round_height = self.current_round_height()?;

        // Check that the chunk lock is currently held by this contributor.
        let round = self.get_round(round_height)?;
        if !round.is_chunk_locked_by(chunk_id, &participant) {
            error!("{} should have lock on chunk {} but does not", &participant, chunk_id);
            return Err(CoordinatorError::ChunkNotLockedOrByWrongParticipant);
        }

        // Check that the contribution locator corresponding to this round and chunk exists.
        if self
            .environment
            .contribution_locator_exists(round_height, chunk_id, contribution_id, false)
        {
            error!("Locator for contribution {} already exists", contribution_id);
            return Err(CoordinatorError::ContributionLocatorAlreadyExists);
        }

        // Fetch the current round and given chunk ID and check that
        // the given contribution ID has not been verified yet.
        let chunk = round.get_chunk(chunk_id)?;
        if chunk.get_contribution(contribution_id).is_ok() {
            return Err(CoordinatorError::ContributionShouldNotExist);
        }

        debug!(
            "Coordinator is starting computation on chunk {} contribution {}",
            chunk_id, contribution_id
        );
        Computation::run(&self.environment, round_height, chunk_id, contribution_id)?;
        debug!(
            "Coordinator completed computation on chunk {} contribution {}",
            chunk_id, contribution_id
        );

        // Attempts to set the current contribution as verified in the current round.
        // self.add_contribution(chunk_id, contributor)?;

        info!("Computed chunk {} contribution {}", chunk_id, contribution_id);
        Ok(())
    }

    ///
    /// Attempts to run aggregation for the current round.
    ///
    #[inline]
    fn run_aggregation(&self) -> Result<(), CoordinatorError> {
        // Fetch the current round.
        let current_round = self.current_round()?;
        let current_round_height = current_round.round_height();

        // Check that all current round chunks are fully contributed and verified.
        if !current_round.is_complete() {
            return Err(CoordinatorError::RoundNotComplete);
        }

        // TODO (howardwu): Do pre-check that all current chunk contributions are present.
        // Check that the round directory corresponding to this round exists.
        if !self.environment.round_directory_exists(current_round_height) {
            return Err(CoordinatorError::RoundDirectoryMissing);
        }

        // TODO (howardwu): Add aggregate verification logic.
        // Execute aggregation to combine on all chunks to finalize the round
        // corresponding to the given round height.
        debug!("Coordinator is starting aggregation");
        Aggregation::run(&self.environment, &current_round)?;
        debug!("Coordinator completed aggregation");

        // Check that the round locator exists.
        if !self.environment.round_locator_exists(current_round_height) {
            return Err(CoordinatorError::RoundLocatorMissing);
        }

        Ok(())
    }

    ///
    /// Updates the round corresponding to the given height in storage.
    ///
    #[inline]
    fn save_round_to_storage(&self, round_height: u64, round: Round) -> Result<(), CoordinatorError> {
        trace!("Writing round {} to storage", round_height);

        // TODO (howardwu): Do we need to structure this entry as an atomic transaction?
        let mut success = false;
        // Acquire the storage write lock.
        // let mut storage = self.storage_mut()?;
        let mut storage = match self.storage.write() {
            Ok(storage) => storage,
            _ => return Err(CoordinatorError::StorageWriterFailed),
        };

        // First, add the new round to storage.
        if storage.insert(Key::Round(round_height), Value::Round(round)) {
            // Next, update the round height to reflect the update.
            if storage.insert(Key::RoundHeight, Value::RoundHeight(round_height)) {
                // Lastly, save the round to storage.
                if storage.save() {
                    debug!("Completed writing round {} to storage", round_height);
                    success = true;
                }
            }
        }

        match success {
            true => Ok(()),
            false => Err(CoordinatorError::StorageUpdateFailed),
        }
    }

    ///
    /// Resets the round if a participant timed out and removes that participant.
    ///
    #[inline]
    fn reset_if_timed_out(&self) -> Result<bool, CoordinatorError> {
        let round = self.current_round()?;
        let current_time = Utc::now();
        let mut faulty_participants = HashSet::new();
        for chunk in round.get_chunks() {
            match chunk.lock_holder() {
                Some((lock_holder, lock_time)) => {
                    if current_time - *lock_time >= self.environment.lock_timeout() {
                        let faulty_participant = lock_holder.clone();
                        faulty_participants.insert(faulty_participant);
                    }
                }
                None => {
                    // This chunk isn't locked so it can't time out.
                }
            }
        }
        let is_round_taking_too_long = match round.started_at() {
            Some(t) => current_time - *t >= self.environment.round_timeout(),
            None => false,
        };
        // Check if there's at least one timed out participant. If so, reset without the faulty
        // participant. Otherwise, do nothing.
        if faulty_participants.len() > 0 || is_round_taking_too_long {
            if faulty_participants.len() > 0 {
                info!(
                    "Performing a round reset without faulty participants: {:?}",
                    faulty_participants
                );
            }
            if is_round_taking_too_long {
                info!("Performing a round reset since timeout has passed");
            }
            let mut participants = vec![];
            for contributor in round.get_contributors() {
                if !faulty_participants.contains(contributor) {
                    participants.push(contributor.clone());
                }
            }
            let verifiers = round.get_verifiers().clone();
            let started_at = Utc::now();
            self.reset_round(
                started_at,
                &format!("timeout_{}", started_at.timestamp()),
                participants,
                verifiers,
            )?;

            Ok(true)
        } else {
            Ok(false)
        }
    }

    ///
    /// Attempts to acquire the read lock for storage.
    ///
    #[inline]
    fn storage(&self) -> Result<RwLockReadGuard<Box<dyn Storage>>, CoordinatorError> {
        match self.storage.read() {
            Ok(storage) => Ok(storage),
            _ => {
                error!("Failed to acquire the storage read lock");
                Err(CoordinatorError::StorageReaderFailed)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{environment::*, testing::prelude::*, Coordinator};

    use chrono::Utc;
    use once_cell::sync::Lazy;

    fn initialize_coordinator(coordinator: &Coordinator) -> anyhow::Result<()> {
        // Ensure the ceremony has not started.
        assert_eq!(0, coordinator.current_round_height()?);

        // Run initialization.
        coordinator.next_round(
            *TEST_STARTED_AT,
            vec![
                Lazy::force(&TEST_CONTRIBUTOR_ID).clone(),
                Lazy::force(&TEST_CONTRIBUTOR_ID_2).clone(),
            ],
            vec![Lazy::force(&TEST_VERIFIER_ID).clone()],
        )?;

        // Check current round height is now 1.
        assert_eq!(1, coordinator.current_round_height()?);
        Ok(())
    }

    fn coordinator_initialization_test() -> anyhow::Result<()> {
        clear_test_transcript();

        let coordinator = Coordinator::new(TEST_ENVIRONMENT.clone())?;

        // Ensure the ceremony has not started.
        assert_eq!(0, coordinator.current_round_height()?);

        // Run initialization.
        coordinator.next_round(
            Utc::now(),
            vec![
                Lazy::force(&TEST_CONTRIBUTOR_ID).clone(),
                Lazy::force(&TEST_CONTRIBUTOR_ID_2).clone(),
            ],
            vec![Lazy::force(&TEST_VERIFIER_ID).clone()],
        )?;

        {
            // Check round 0 is complete.
            assert!(coordinator.get_round(0)?.is_complete());

            // Check current round height is now 1.
            assert_eq!(1, coordinator.current_round_height()?);

            // Check round 1 contributors.
            assert_eq!(2, coordinator.current_round()?.number_of_contributors());
            assert!(coordinator.is_current_contributor(&TEST_CONTRIBUTOR_ID));
            assert!(coordinator.is_current_contributor(&TEST_CONTRIBUTOR_ID_2));
            assert!(!coordinator.is_current_contributor(&TEST_CONTRIBUTOR_ID_3));
            assert!(!coordinator.is_current_contributor(&TEST_VERIFIER_ID));

            // Check round 1 verifiers.
            assert_eq!(1, coordinator.current_round()?.get_verifiers().len());
            assert!(coordinator.is_current_verifier(&TEST_VERIFIER_ID));
            assert!(!coordinator.is_current_verifier(&TEST_VERIFIER_ID_2));
            assert!(!coordinator.is_current_verifier(&TEST_CONTRIBUTOR_ID));

            // Check round 1 is NOT complete.
            assert!(!coordinator.current_round()?.is_complete());
        }

        Ok(())
    }

    fn coordinator_contributor_try_lock_test() -> anyhow::Result<()> {
        clear_test_transcript();

        let coordinator = Coordinator::new(TEST_ENVIRONMENT.clone())?;
        initialize_coordinator(&coordinator)?;

        {
            // Acquire the lock for chunk 0 as contributor 1.
            let contributor = Lazy::force(&TEST_CONTRIBUTOR_ID);
            assert!(coordinator.try_lock_chunk(0, contributor.clone()).is_ok());

            // Attempt to acquire the lock for chunk 0 again.
            assert!(coordinator.try_lock_chunk(0, contributor.clone()).is_err());

            // Attempt to acquire the lock for chunk 1.
            assert!(coordinator.try_lock_chunk(1, contributor.clone()).is_err());

            // Attempt to acquire the lock for chunk 0 as contributor 2.
            let contributor_2 = Lazy::force(&TEST_CONTRIBUTOR_ID_2).clone();
            assert!(coordinator.try_lock_chunk(0, contributor_2.clone()).is_err());

            // Attempt to acquire the lock for chunk 1 as contributor 2.
            assert!(coordinator.try_lock_chunk(1, contributor_2).is_ok());
        }

        {
            // Check that chunk 0 is locked.
            let chunk_id = 0;
            let round = coordinator.current_round()?;
            let chunk = round.get_chunk(chunk_id)?;
            assert!(chunk.is_locked());
            assert!(!chunk.is_unlocked());

            // Check that chunk 0 is locked by contributor 1.
            let contributor = Lazy::force(&TEST_CONTRIBUTOR_ID);
            assert!(chunk.is_locked_by(contributor));

            // Check that chunk 1 is locked.
            let chunk_id = 1;
            let chunk = round.get_chunk(chunk_id)?;
            assert!(chunk.is_locked());
            assert!(!chunk.is_unlocked());

            // Check that chunk 0 is locked by contributor 2.
            let contributor_2 = Lazy::force(&TEST_CONTRIBUTOR_ID_2);
            assert!(chunk.is_locked_by(contributor_2));
            assert!(!chunk.is_locked_by(contributor));
        }

        Ok(())
    }

    fn coordinator_contributor_add_contribution_test() -> anyhow::Result<()> {
        clear_test_transcript();

        let coordinator = Coordinator::new(TEST_ENVIRONMENT_3.clone())?;
        initialize_coordinator(&coordinator)?;

        // Acquire the lock for chunk 0 as contributor 1.
        let contributor = Lazy::force(&TEST_CONTRIBUTOR_ID);
        coordinator.try_lock_chunk(0, contributor.clone())?;

        // Run computation on round 1 chunk 0 contribution 1.
        {
            // Check current round is 1.
            let round = coordinator.current_round()?;
            assert_eq!(1, round.round_height());

            // Check chunk 0 is not verified.
            let chunk_id = 0;
            let chunk = round.get_chunk(chunk_id)?;
            assert!(!chunk.is_complete(round.expected_num_contributions()));

            // Check next contribution is 1.
            let contribution_id = 1;
            assert!(chunk.is_next_contribution_id(contribution_id, round.expected_num_contributions()));

            // Run the computation
            assert!(
                coordinator
                    .run_computation(chunk_id, contribution_id, &contributor)
                    .is_ok()
            );
        }

        // Add contribution for round 1 chunk 0 contribution 1.
        {
            // Add round 1 chunk 0 contribution 1.
            let chunk_id = 0;
            assert!(coordinator.add_contribution(chunk_id, contributor.clone()).is_ok());

            // Check chunk 0 lock is released.
            let round = coordinator.current_round()?;
            let chunk = round.get_chunk(chunk_id)?;
            assert!(chunk.is_unlocked());
            assert!(!chunk.is_locked());
        }

        Ok(())
    }

    fn coordinator_verifier_verify_contribution_test() -> anyhow::Result<()> {
        clear_test_transcript();

        let coordinator = Coordinator::new(TEST_ENVIRONMENT_3.clone())?;
        initialize_coordinator(&coordinator)?;

        // Acquire the lock for chunk 0 as contributor 1.
        let chunk_id = 0;
        let contributor = Lazy::force(&TEST_CONTRIBUTOR_ID);
        assert!(coordinator.try_lock_chunk(chunk_id, contributor.clone()).is_ok());

        // Run computation on round 1 chunk 0 contribution 1.
        let contribution_id = 1;
        assert!(
            coordinator
                .run_computation(chunk_id, contribution_id, contributor)
                .is_ok()
        );

        // Add round 1 chunk 0 contribution 1.
        assert!(coordinator.add_contribution(chunk_id, contributor.clone()).is_ok());

        // Acquire lock for round 1 chunk 0 contribution 1.
        {
            // Acquire the lock on chunk 0 for the verifier.
            let verifier = Lazy::force(&TEST_VERIFIER_ID).clone();
            assert!(coordinator.try_lock_chunk(chunk_id, verifier.clone()).is_ok());

            // Check that chunk 0 is locked.
            let round = coordinator.current_round()?;
            let chunk = round.get_chunk(chunk_id)?;
            assert!(chunk.is_locked());
            assert!(!chunk.is_unlocked());

            // Check that chunk 0 is locked by the verifier.
            assert!(chunk.is_locked_by(&verifier));
        }

        // Verify round 1 chunk 0 contribution 1.
        {
            // Verify contribution 1.
            let verifier = Lazy::force(&TEST_VERIFIER_ID).clone();
            coordinator.verify_contribution(chunk_id, verifier)?;
        }

        Ok(())
    }

    // TODO (howardwu): Update and finish this test to reflect new compressed output setting.
    fn coordinator_aggregation_test() -> anyhow::Result<()> {
        clear_test_transcript();

        let coordinator = Coordinator::new(TEST_ENVIRONMENT_3.clone())?;
        initialize_coordinator(&coordinator)?;

        // Run computation and verification on each contribution in each chunk.
        let contributors = vec![
            Lazy::force(&TEST_CONTRIBUTOR_ID).clone(),
            Lazy::force(&TEST_CONTRIBUTOR_ID_2).clone(),
        ];

        let verifier = Lazy::force(&TEST_VERIFIER_ID);

        // Iterate over all chunk IDs.
        for chunk_id in 0..TEST_ENVIRONMENT_3.number_of_chunks() {
            // As contribution ID 0 is initialized by the coordinator, iterate from
            // contribution ID 1 up to the expected number of contributions.
            for contribution_id in 1..coordinator.current_round()?.expected_num_contributions() {
                let contributor = &contributors[contribution_id as usize - 1];
                {
                    // Acquire the lock as contributor.
                    let try_lock = coordinator.try_lock_chunk(chunk_id, contributor.clone());
                    if try_lock.is_err() {
                        println!(
                            "Failed to acquire lock as contributor {:?}\n{}",
                            contributor.clone(),
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                        try_lock?;
                    }

                    // Run computation as contributor.
                    let contribute = coordinator.run_computation(chunk_id, contribution_id, &contributor);
                    if contribute.is_err() {
                        println!(
                            "Failed to run computation as contributor {:?}\n{}",
                            contributor.clone(),
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                        contribute?;
                    }

                    // Add the contribution as the contributor.
                    let contribute = coordinator.add_contribution(chunk_id, contributor.clone());
                    if contribute.is_err() {
                        println!(
                            "Failed to add contribution as contributor {:?}\n{}",
                            contributor.clone(),
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                        contribute?;
                    }
                }
                {
                    // Acquire the lock as the verifier.
                    let try_lock = coordinator.try_lock_chunk(chunk_id, verifier.clone());
                    if try_lock.is_err() {
                        println!(
                            "Failed to acquire lock as verifier {:?}\n{}",
                            contributor.clone(),
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                        try_lock?;
                    }

                    // Run verification as the verifier.
                    let verify = coordinator.verify_contribution(chunk_id, verifier.clone());
                    if verify.is_err() {
                        println!(
                            "Failed to run verification as verifier {:?}\n{}",
                            contributor.clone(),
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                        verify?;
                    }
                }
            }
        }

        Ok(())
    }

    fn coordinator_next_round_test() -> anyhow::Result<()> {
        clear_test_transcript();

        let coordinator = Coordinator::new(TEST_ENVIRONMENT_3.clone())?;
        let contributor = Lazy::force(&TEST_CONTRIBUTOR_ID).clone();
        let verifier = Lazy::force(&TEST_VERIFIER_ID);
        {
            // Ensure the ceremony has not started.
            assert_eq!(0, coordinator.current_round_height()?);
            // Run initialization.
            coordinator.next_round(*TEST_STARTED_AT, vec![contributor.clone()], vec![verifier.clone()])?;
            // Check current round height is now 1.
            assert_eq!(1, coordinator.current_round_height()?);
        }

        // Run computation and verification on each contribution in each chunk.
        for chunk_id in 0..TEST_ENVIRONMENT_3.number_of_chunks() {
            // Ensure contribution ID 0 is already verified by the coordinator.
            assert!(
                coordinator
                    .current_round()?
                    .get_chunk(chunk_id)?
                    .get_contribution(0)?
                    .is_verified()
            );

            // As contribution ID 0 is initialized by the coordinator, iterate from
            // contribution ID 1 up to the expected number of contributions.
            for contribution_id in 1..coordinator.current_round()?.expected_num_contributions() {
                {
                    // Acquire the lock as contributor.
                    if coordinator.try_lock_chunk(chunk_id, contributor.clone()).is_err() {
                        panic!(
                            "Failed to acquire lock as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Run computation as contributor.
                    if coordinator
                        .run_computation(chunk_id, contribution_id, &contributor)
                        .is_err()
                    {
                        panic!(
                            "Failed to run computation as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Add the contribution as the contributor.
                    if coordinator.add_contribution(chunk_id, contributor.clone()).is_err() {
                        panic!(
                            "Failed to add contribution as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                }
                {
                    // Acquire the lock as the verifier.
                    if coordinator.try_lock_chunk(chunk_id, verifier.clone()).is_err() {
                        panic!(
                            "Failed to acquire lock as verifier {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Run verification as the verifier.
                    if coordinator.verify_contribution(chunk_id, verifier.clone()).is_err() {
                        panic!(
                            "Failed to run verification as verifier {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                }
            }
        }

        println!(
            "Starting aggregation with this transcript {}",
            serde_json::to_string_pretty(&coordinator.current_round()?)?
        );

        // Run aggregation and transition from round 1 to round 2.
        coordinator.next_round(Utc::now(), vec![contributor.clone()], vec![verifier.clone()])?;

        println!(
            "Finished aggregation with this transcript {}",
            serde_json::to_string_pretty(&coordinator.current_round()?)?
        );

        Ok(())
    }

    fn coordinator_reset_round_test() -> anyhow::Result<()> {
        test_logger();
        clear_test_transcript();

        let coordinator = Coordinator::new(TEST_ENVIRONMENT_3.clone())?;
        let contributor = Lazy::force(&TEST_CONTRIBUTOR_ID).clone();
        let contributor2 = Lazy::force(&TEST_CONTRIBUTOR_ID_2).clone();
        let verifier = Lazy::force(&TEST_VERIFIER_ID);
        coordinator.next_round(*TEST_STARTED_AT, vec![contributor.clone(), contributor2.clone()], vec![
            verifier.clone(),
        ])?;
        // Run computation and verification on each contribution in each chunk.
        for chunk_id in 0..TEST_ENVIRONMENT_3.number_of_chunks() {
            // Ensure contribution ID 0 is already verified by the coordinator.
            assert!(
                coordinator
                    .current_round()?
                    .get_chunk(chunk_id)?
                    .get_contribution(0)?
                    .is_verified()
            );

            // As contribution ID 0 is initialized by the coordinator, iterate from
            // contribution ID 1 up to the expected number of contributions minus 1.
            for contribution_id in 1..(coordinator.current_round()?.expected_num_contributions() - 1) {
                {
                    // Acquire the lock as contributor.
                    if coordinator.try_lock_chunk(chunk_id, contributor.clone()).is_err() {
                        panic!(
                            "Failed to acquire lock as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Run computation as contributor.
                    if coordinator
                        .run_computation(chunk_id, contribution_id, &contributor)
                        .is_err()
                    {
                        panic!(
                            "Failed to run computation as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Add the contribution as the contributor.
                    if coordinator.add_contribution(chunk_id, contributor.clone()).is_err() {
                        panic!(
                            "Failed to add contribution as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                }
                {
                    // Acquire the lock as the verifier.
                    if coordinator.try_lock_chunk(chunk_id, verifier.clone()).is_err() {
                        panic!(
                            "Failed to acquire lock as verifier {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Run verification as the verifier.
                    if coordinator.verify_contribution(chunk_id, verifier.clone()).is_err() {
                        panic!(
                            "Failed to run verification as verifier {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                }
            }
        }

        // Reset an incomplete round.
        {
            assert_eq!(1, coordinator.current_round_height()?);
            // Run initialization.
            let backup_tag = Utc::now().timestamp_nanos().to_string();
            coordinator.reset_round(*TEST_STARTED_AT, &backup_tag, vec![contributor2.clone()], vec![
                verifier.clone(),
            ])?;
            assert_eq!(1, coordinator.current_round_height()?);
            let backup_path = coordinator.environment.round_backup_directory(1, &backup_tag);
            assert!(std::path::Path::new(&backup_path).exists())
        }

        for chunk_id in 0..TEST_ENVIRONMENT_3.number_of_chunks() {
            // Ensure contribution ID 0 is already verified by the coordinator.
            assert!(
                coordinator
                    .current_round()?
                    .get_chunk(chunk_id)?
                    .get_contribution(0)?
                    .is_verified()
            );

            // As contribution ID 0 is initialized by the coordinator, iterate from
            // contribution ID 1 up to the expected number of contributions.
            for contribution_id in 1..coordinator.current_round()?.expected_num_contributions() {
                {
                    // Acquire the lock as contributor.
                    if coordinator.try_lock_chunk(chunk_id, contributor2.clone()).is_err() {
                        panic!(
                            "Failed to acquire lock as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Run computation as contributor.
                    if coordinator
                        .run_computation(chunk_id, contribution_id, &contributor2)
                        .is_err()
                    {
                        panic!(
                            "Failed to run computation as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Add the contribution as the contributor.
                    if coordinator.add_contribution(chunk_id, contributor2.clone()).is_err() {
                        panic!(
                            "Failed to add contribution as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                }
                {
                    // Acquire the lock as the verifier.
                    if coordinator.try_lock_chunk(chunk_id, verifier.clone()).is_err() {
                        panic!(
                            "Failed to acquire lock as verifier {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Run verification as the verifier.
                    if coordinator.verify_contribution(chunk_id, verifier.clone()).is_err() {
                        panic!(
                            "Failed to run verification as verifier {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                }
            }
        }

        // Reset a complete round.
        {
            assert_eq!(1, coordinator.current_round_height()?);
            // Run initialization.
            let backup_tag = Utc::now().timestamp_nanos().to_string();
            coordinator.reset_round(*TEST_STARTED_AT, &backup_tag, vec![contributor.clone()], vec![
                verifier.clone(),
            ])?;
            assert_eq!(1, coordinator.current_round_height()?);
            let backup_path = coordinator.environment.round_backup_directory(1, &backup_tag);
            assert!(std::path::Path::new(&backup_path).exists())
        }

        for chunk_id in 0..TEST_ENVIRONMENT_3.number_of_chunks() {
            // Ensure contribution ID 0 is already verified by the coordinator.
            assert!(
                coordinator
                    .current_round()?
                    .get_chunk(chunk_id)?
                    .get_contribution(0)?
                    .is_verified()
            );

            // As contribution ID 0 is initialized by the coordinator, iterate from
            // contribution ID 1 up to the expected number of contributions.
            for contribution_id in 1..coordinator.current_round()?.expected_num_contributions() {
                {
                    // Acquire the lock as contributor.
                    if coordinator.try_lock_chunk(chunk_id, contributor.clone()).is_err() {
                        panic!(
                            "Failed to acquire lock as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Run computation as contributor.
                    if coordinator
                        .run_computation(chunk_id, contribution_id, &contributor)
                        .is_err()
                    {
                        panic!(
                            "Failed to run computation as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Add the contribution as the contributor.
                    if coordinator.add_contribution(chunk_id, contributor.clone()).is_err() {
                        panic!(
                            "Failed to add contribution as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                }
                {
                    // Acquire the lock as the verifier.
                    if coordinator.try_lock_chunk(chunk_id, verifier.clone()).is_err() {
                        panic!(
                            "Failed to acquire lock as verifier {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Run verification as the verifier.
                    if coordinator.verify_contribution(chunk_id, verifier.clone()).is_err() {
                        panic!(
                            "Failed to run verification as verifier {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                }
            }
        }

        Ok(())
    }

    fn coordinator_reset_round_timeout() -> anyhow::Result<()> {
        test_logger();
        clear_test_transcript();

        let coordinator = Coordinator::new(TEST_ENVIRONMENT_3.clone())?;
        let contributor = Lazy::force(&TEST_CONTRIBUTOR_ID).clone();
        let contributor2 = Lazy::force(&TEST_CONTRIBUTOR_ID_2).clone();
        let verifier = Lazy::force(&TEST_VERIFIER_ID);
        coordinator.next_round(*TEST_STARTED_AT, vec![contributor.clone(), contributor2.clone()], vec![
            verifier.clone(),
        ])?;
        // Run computation and verification on each contribution in each chunk.
        for chunk_id in 0..TEST_ENVIRONMENT_3.number_of_chunks() {
            // Ensure contribution ID 0 is already verified by the coordinator.
            assert!(
                coordinator
                    .current_round()?
                    .get_chunk(chunk_id)?
                    .get_contribution(0)?
                    .is_verified()
            );

            // As contribution ID 0 is initialized by the coordinator, iterate from
            // contribution ID 1 up to the expected number of contributions minus 1.
            for contribution_id in 1..(coordinator.current_round()?.expected_num_contributions() - 1) {
                {
                    // Acquire the lock as contributor.
                    if coordinator.try_lock_chunk(chunk_id, contributor.clone()).is_err() {
                        panic!(
                            "Failed to acquire lock as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Run computation as contributor.
                    if coordinator
                        .run_computation(chunk_id, contribution_id, &contributor)
                        .is_err()
                    {
                        panic!(
                            "Failed to run computation as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Add the contribution as the contributor.
                    if coordinator.add_contribution(chunk_id, contributor.clone()).is_err() {
                        panic!(
                            "Failed to add contribution as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                }
                {
                    // Acquire the lock as the verifier.
                    if coordinator.try_lock_chunk(chunk_id, verifier.clone()).is_err() {
                        panic!(
                            "Failed to acquire lock as verifier {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Run verification as the verifier.
                    if coordinator.verify_contribution(chunk_id, verifier.clone()).is_err() {
                        panic!(
                            "Failed to run verification as verifier {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                }
            }
        }

        std::thread::sleep(coordinator.environment.round_timeout().to_std()?);
        let was_reset = coordinator.reset_if_timed_out()?;
        assert!(was_reset);
        let current_round = coordinator.current_round()?;
        let current_participants = current_round.get_contributors();
        assert_eq!(current_participants.len(), 2);

        if coordinator.try_lock_chunk(0, contributor.clone()).is_err() {
            panic!(
                "Failed to acquire lock as contributor {}",
                serde_json::to_string_pretty(&coordinator.current_round()?)?
            );
        }
        std::thread::sleep(coordinator.environment.lock_timeout().to_std()?);
        let was_reset = coordinator.reset_if_timed_out()?;
        assert!(was_reset);
        let current_round = coordinator.current_round()?;
        let current_participants = current_round.get_contributors();
        // Check that contributor has been removed.
        assert_eq!(current_participants.len(), 1);
        assert_eq!(current_participants[0], contributor2);
        Ok(())
    }

    fn coordinator_reset_round_aggregation_failure() -> anyhow::Result<()> {
        test_logger();
        clear_test_transcript();

        let coordinator = Coordinator::new(TEST_ENVIRONMENT_3.clone())?;
        let contributor = Lazy::force(&TEST_CONTRIBUTOR_ID).clone();
        let verifier = Lazy::force(&TEST_VERIFIER_ID);
        coordinator.next_round(*TEST_STARTED_AT, vec![contributor.clone()], vec![verifier.clone()])?;
        // Run computation and verification on each contribution in each chunk.
        for chunk_id in 0..TEST_ENVIRONMENT_3.number_of_chunks() {
            // Ensure contribution ID 0 is already verified by the coordinator.
            assert!(
                coordinator
                    .current_round()?
                    .get_chunk(chunk_id)?
                    .get_contribution(0)?
                    .is_verified()
            );

            // As contribution ID 0 is initialized by the coordinator, iterate from
            // contribution ID 1 up to the expected number of contributions minus 1.
            for contribution_id in 1..coordinator.current_round()?.expected_num_contributions() {
                {
                    // Acquire the lock as contributor.
                    if coordinator.try_lock_chunk(chunk_id, contributor.clone()).is_err() {
                        panic!(
                            "Failed to acquire lock as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Run computation as contributor.
                    if coordinator
                        .run_computation(chunk_id, contribution_id, &contributor)
                        .is_err()
                    {
                        panic!(
                            "Failed to run computation as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Add the contribution as the contributor.
                    if coordinator.add_contribution(chunk_id, contributor.clone()).is_err() {
                        panic!(
                            "Failed to add contribution as contributor {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                }
                {
                    // Acquire the lock as the verifier.
                    if coordinator.try_lock_chunk(chunk_id, verifier.clone()).is_err() {
                        panic!(
                            "Failed to acquire lock as verifier {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }
                    // Run verification as the verifier.
                    if coordinator.verify_contribution(chunk_id, verifier.clone()).is_err() {
                        panic!(
                            "Failed to run verification as verifier {}",
                            serde_json::to_string_pretty(&coordinator.current_round()?)?
                        );
                    }

                    if chunk_id == 1 {
                        let chunk_0_location =
                            coordinator
                                .environment
                                .contribution_locator(1, 0, contribution_id, false);
                        let chunk_1_location =
                            coordinator
                                .environment
                                .contribution_locator(1, 1, contribution_id, false);
                        std::fs::remove_file(chunk_1_location.clone())?;
                        std::fs::copy(chunk_0_location, chunk_1_location)?;
                    }
                }
            }
        }

        let next_round = coordinator.next_round(Utc::now(), vec![contributor.clone()], vec![verifier.clone()])?;
        // That means that moving to the next round succeeded, which it shouldn't.
        if next_round != 1 {
            panic!("Should have remained on the same round since aggregation failred.");
        }

        Ok(())
    }

    #[test]
    #[serial]
    fn test_coordinator_initialization_matches_json() {
        clear_test_transcript();

        let coordinator = Coordinator::new(TEST_ENVIRONMENT.clone()).unwrap();
        initialize_coordinator(&coordinator).unwrap();

        // Check that round 0 matches the round 0 JSON specification.
        {
            // Fetch round 0 from coordinator.
            let expected = test_round_0_json().unwrap();
            let candidate = coordinator.get_round(0).unwrap();
            print_diff(&expected, &candidate);
            assert_eq!(expected, candidate);
        }
    }

    #[test]
    #[serial]
    fn test_coordinator_initialization() {
        coordinator_initialization_test().unwrap();
    }

    #[test]
    #[serial]
    fn test_coordinator_contributor_try_lock() {
        coordinator_contributor_try_lock_test().unwrap();
    }

    #[test]
    #[serial]
    fn test_coordinator_contributor_add_contribution() {
        coordinator_contributor_add_contribution_test().unwrap();
    }

    #[test]
    #[serial]
    fn test_coordinator_contributor_verify_contribution() {
        coordinator_verifier_verify_contribution_test().unwrap();
    }

    #[test]
    #[serial]
    fn test_coordinator_aggregation() {
        coordinator_aggregation_test().unwrap();
    }

    #[test]
    #[serial]
    fn test_coordinator_next_round() {
        coordinator_next_round_test().unwrap();
    }

    #[test]
    #[serial]
    fn test_coordinator_reset_round() {
        coordinator_reset_round_test().unwrap();
    }

    #[test]
    #[serial]
    fn test_coordinator_reset_timeout() {
        coordinator_reset_round_timeout().unwrap();
    }

    #[test]
    #[serial]
    fn test_coordinator_reset_aggregation_failure() {
        coordinator_reset_round_aggregation_failure().unwrap();
    }

    #[test]
    #[serial]
    fn test_coordinator_number_of_chunks() {
        clear_test_transcript();

        let environment = Environment::Test(Parameters::AleoTestChunks(4096));

        let coordinator = Coordinator::new(environment.clone()).unwrap();
        initialize_coordinator(&coordinator).unwrap();

        assert_eq!(
            environment.number_of_chunks(),
            coordinator.get_round(0).unwrap().get_chunks().len() as u64
        );
    }
}
