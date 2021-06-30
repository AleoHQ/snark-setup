use crate::{
    errors::VerifierError,
    objects::LockResponse,
    tasks::Tasks,
    utils::{authentication::AleoAuthentication, create_parent_directory, remove_file_if_exists, write_to_file},
};

use phase1::helpers::CurveKind;
use phase1_cli::transform_pok_and_correctness;
use phase1_coordinator::{
    environment::Environment,
    objects::{ContributionFileSignature, ContributionState},
    phase1_chunked_parameters,
    Participant,
};
use setup_utils::calculate_hash;
use snarkos_toolkit::account::{Address, ViewKey};
use snarkvm_curves::{bls12_377::Bls12_377, bw6_761::BW6_761};

use chrono::Utc;
use std::{fs, str::FromStr, sync::Arc, thread::sleep, time::Duration};
use tokio::{signal, sync::Mutex};
use tracing::{debug, error, info, trace, warn};
use url::Url;

/// Returns a pretty print of the given hash bytes for logging.
macro_rules! pretty_hash {
    ($hash:expr) => {{
        let mut output = format!("\n\n");
        for line in $hash.chunks(16) {
            output += "\t";
            for section in line.chunks(4) {
                for b in section {
                    output += &format!("{:02x}", b);
                }
                output += " ";
            }
            output += "\n";
        }
        output
    }};
}

///
/// The verifier used to manage and dispatch/execute verifier operations
/// to the remote coordinator.
///
#[derive(Debug)]
pub struct Verifier {
    /// The url of the coordinator that will be
    pub(crate) coordinator_api_url: Url,

    /// The view key that will be used for server authentication
    pub(crate) view_key: ViewKey,

    /// The identity of the verifier
    pub(crate) verifier: Participant,

    /// The coordinator environment
    pub(crate) environment: Environment,

    /// The list of cached tasks.
    pub(crate) tasks: Arc<Mutex<Tasks>>,

    /// The path where tasks will be stored.
    pub(crate) tasks_storage_path: String,
}

// Manual implementation, since ViewKey doesn't implement Clone
impl Clone for Verifier {
    fn clone(&self) -> Self {
        let view_key = ViewKey::from_str(&self.view_key.to_string()).expect("Error cloning the verifier ViewKey");
        Self {
            coordinator_api_url: self.coordinator_api_url.clone(),
            view_key,
            verifier: self.verifier.clone(),
            environment: self.environment.clone(),
            tasks: self.tasks.clone(),
            tasks_storage_path: self.tasks_storage_path.clone(),
        }
    }
}

impl Verifier {
    ///
    /// Initialize a new verifier.
    ///
    pub fn new(
        coordinator_api_url: Url,
        view_key: ViewKey,
        address: Address,
        environment: Environment,
        tasks_storage_path: String,
    ) -> Result<Self, VerifierError> {
        let verifier_id = address.to_string();

        Ok(Self {
            coordinator_api_url,
            view_key,
            verifier: Participant::Verifier(verifier_id),
            environment,
            tasks: Arc::new(Mutex::new(Tasks::load(&tasks_storage_path))),
            tasks_storage_path,
        })
    }

    ///
    /// Initializes a listener to handle the shutdown signal.
    ///
    #[inline]
    pub async fn shutdown_listener(self) -> anyhow::Result<()> {
        signal::ctrl_c().await.expect("Failed to listen for control+c");

        warn!("\n\nATTENTION - Verifier is shutting down...\n");

        // Acquire the tasks lock.
        let tasks = self.tasks.lock().await;
        trace!("Verifier has acquired the tasks lock");

        // Store the verifier tasks to disk.
        tasks.store(&self.tasks_storage_path)?;

        // Print the verifier tasks
        let tasks = serde_json::to_string_pretty(&*tasks).unwrap();

        info!("\n\nVerifier tasks at Shutdown\n\n{}\n", tasks);
        info!("\n\nVerifier has safely shutdown.\n\nGoodbye.\n");

        std::process::exit(0);
    }

    ///
    /// The function attempts to fetch a task from the queue. If there are
    /// no tasks in the queue, then the verifier will request a lock from
    /// the coordinator.
    ///
    #[inline]
    pub async fn get_task(&self) -> Result<LockResponse, VerifierError> {
        // Acquire the tasks lock.
        let mut tasks = self.tasks.lock().await;

        // Attempt to fetch a task or lock a chunk.
        let lock_response = match tasks.next_task() {
            Some(lock_response) => lock_response,
            None => {
                let task = self.lock_chunk().await?;
                tasks.add_task(task.clone());
                task
            }
        };

        // Write tasks to disk.
        tasks.store(&self.tasks_storage_path)?;

        Ok(lock_response)
    }

    ///
    /// Clear a task from the queue. If the queue is empty, clear the storage.
    ///
    #[inline]
    pub async fn clear_task(&self, task: &LockResponse) -> Result<(), VerifierError> {
        // Acquire the tasks lock.
        let mut tasks = self.tasks.lock().await;

        // Remove the given task from `tasks`.
        tasks.remove_task(task);

        if tasks.is_empty() {
            // If there are no tasks, delete the stored tasks file.
            remove_file_if_exists(&self.tasks_storage_path);
        } else {
            // Otherwise, update the stored file
            tasks.store(&self.tasks_storage_path)?;
        }

        Ok(())
    }

    ///
    /// Downloads the challenge file from the coordinator and stores it to the verifier filesystem.
    /// Returns the hash of the downloaded response file. Otherwise, returns a `VerifierError`
    ///
    pub async fn process_challenge_file(&self, challenge_locator: &str) -> Result<Vec<u8>, VerifierError> {
        // Download the challenge file from the coordinator.
        let challenge_file = self.download_challenge_file(&challenge_locator).await?;

        // Compute the challenge hash using the challenge file.
        let challenge_hash = calculate_hash(&challenge_file);

        debug!(
            "Writing the challenge file (size: {}) {} to disk",
            challenge_file.len(),
            &challenge_locator
        );

        // Write the challenge file to disk.
        write_to_file(&challenge_locator, challenge_file);

        debug!("The challenge hash is {}", pretty_hash!(&challenge_hash));

        Ok(challenge_hash.to_vec())
    }

    ///
    /// Downloads the response file from the coordinator and stores it to the verifier filesystem.
    /// Returns the hash of the downloaded response file. Otherwise, returns a `VerifierError`
    ///
    pub async fn process_response_file(&self, response_locator: &str) -> Result<Vec<u8>, VerifierError> {
        // Download the response file from the coordinator.
        let response_file = self.download_response_file(&response_locator).await?;

        // Compute the response hash using the response file.
        let response_hash = calculate_hash(&response_file);

        debug!(
            "Writing the response file (size: {}) {} to disk",
            response_file.len(),
            &response_locator
        );

        // Write the response to a local file
        write_to_file(&response_locator, response_file);

        debug!("The response hash is {}", pretty_hash!(&response_hash));

        Ok(response_hash.to_vec())
    }

    ///
    /// Returns the next challenge file and the hash of the next challenge file at the given locator.
    /// Otherwise, returns a `VerifierError`
    ///
    pub async fn read_next_challenge_file(
        &self,
        next_challenge_locator: &str,
    ) -> Result<(Vec<u8>, Vec<u8>), VerifierError> {
        info!("Reading the next challenge locator at {}", &next_challenge_locator);

        let next_challenge_file = fs::read(&next_challenge_locator)?;

        // Compute the next challenge hash using the next challenge file.
        let next_challenge_hash = calculate_hash(&next_challenge_file);

        debug!("The next challenge hash is {}", pretty_hash!(&next_challenge_hash));

        Ok((next_challenge_file, next_challenge_hash.to_vec()))
    }

    ///
    /// Performs verification on a contribution with the given chunk id and file locators.
    /// Returns the time (in milliseconds) it took for verification to execute.
    ///
    pub fn run_verification(
        &self,
        chunk_id: u64,
        challenge_file_locator: &str,
        response_locator: &str,
        next_challenge_locator: &str,
    ) -> i64 {
        // Create the parent directory for the `next_challenge_locator` if it doesn't already exist.
        create_parent_directory(&next_challenge_locator);
        // Remove the `next_challenge_locator` if it already exists.
        remove_file_if_exists(&next_challenge_locator);

        let settings = self.environment.parameters();

        let compressed_challenge = self.environment.compressed_inputs();
        let compressed_response = self.environment.compressed_outputs();

        info!("Running verification on chunk {}", chunk_id);

        let start = Utc::now();
        match settings.curve() {
            CurveKind::Bls12_377 => transform_pok_and_correctness(
                compressed_challenge,
                &challenge_file_locator,
                compressed_response,
                &response_locator,
                compressed_challenge,
                &next_challenge_locator,
                &phase1_chunked_parameters!(Bls12_377, settings, chunk_id),
            ),
            CurveKind::BW6 => transform_pok_and_correctness(
                compressed_challenge,
                &challenge_file_locator,
                compressed_response,
                &response_locator,
                compressed_challenge,
                &next_challenge_locator,
                &phase1_chunked_parameters!(BW6_761, settings, chunk_id),
            ),
        };
        let stop = Utc::now();

        let contribution_duration = stop.timestamp_millis() - start.timestamp_millis();

        info!(
            "Verification on chunk {} completed in {} seconds",
            chunk_id,
            contribution_duration / 1000
        );

        contribution_duration
    }

    ///
    /// Verifies that the saved response hash in the challenge file is equivalent
    /// to the contribution response hash.
    ///
    pub fn verify_response_hash(&self, next_challenge_file: &[u8], response_hash: &[u8]) -> Result<(), VerifierError> {
        info!("Check that the response hash matches the next challenge hash");

        // Fetch the saved response hash in the next challenge file.
        let saved_response_hash = match next_challenge_file.chunks(64).next() {
            Some(hash) => hash.to_vec(),
            None => return Err(VerifierError::MissingStoredResponseHash),
        };

        // Check that the response hash matches the next challenge hash.
        debug!("The response hash is {}", pretty_hash!(&response_hash));
        debug!("The saved response hash is {}", pretty_hash!(&saved_response_hash));
        if response_hash != saved_response_hash {
            error!("Response hash does not match the saved response hash.");
            return Err(VerifierError::MismatchedResponseHashes);
        }

        Ok(())
    }

    ///
    /// Signs and returns the contribution file signature.
    ///
    pub fn sign_contribution_data(
        &self,
        challenge_hash: &[u8],
        response_hash: &[u8],
        next_challenge_hash: &[u8],
    ) -> Result<ContributionFileSignature, VerifierError> {
        info!("Signing contribution data");

        let contribution_state = ContributionState::new(
            challenge_hash.to_vec(),
            response_hash.to_vec(),
            Some(next_challenge_hash.to_vec()),
        )?;
        let message = contribution_state.signature_message()?;

        let signature = AleoAuthentication::sign(&self.view_key, message)?;

        let contribution_file_signature = ContributionFileSignature::new(signature, contribution_state)?;

        info!("Successfully signed contribution data");

        Ok(contribution_file_signature)
    }

    ///
    /// Returns the serialized signature and next challenge file.
    ///
    pub fn serialize_contribution_and_signature(
        &self,
        challenge_hash: Vec<u8>,
        response_hash: Vec<u8>,
        next_challenge_hash: Vec<u8>,
        next_challenge_file: Vec<u8>,
    ) -> Result<Vec<u8>, VerifierError> {
        // Construct the signed contribution data.
        let signed_contribution_data = self.sign_contribution_data(
            challenge_hash.as_slice(),
            response_hash.as_slice(),
            next_challenge_hash.as_slice(),
        )?;

        // Concatenate the signed contribution data and next challenge file.
        let verifier_flag = vec![1];
        let signature_bytes = hex::decode(signed_contribution_data.get_signature())?;

        let signature_and_next_challenge_bytes = [
            verifier_flag,
            signature_bytes,
            challenge_hash,
            response_hash,
            next_challenge_hash,
            next_challenge_file,
        ]
        .concat();

        Ok(signature_and_next_challenge_bytes)
    }

    ///
    /// Start the verifier loop. Polls the coordinator to lock and verify chunks.
    ///
    /// After completion or error, the loop waits 5 seconds and starts again.
    ///
    pub async fn start_verifier(&self) {
        // Initialize the shutdown listener
        let verifier = self.clone();
        tokio::task::spawn(async move {
            let _ = verifier.shutdown_listener().await;
        });

        // Initialize the verifier loop.
        loop {
            // Run the verification operations.
            if let Err(error) = self.try_verify().await {
                error!("{}", error);
            }

            // Sleep for 5 seconds in between iterations.
            sleep(Duration::from_secs(5));
        }
    }

    ///
    ///  Runs a set of operations to perform verification on a chunk.
    ///
    /// 1. Attempts to lock a chunk
    /// 2. Downloads the challenge file from the coordinator
    /// 3. Downloads the response file from the coordinator
    /// 4. Runs the verification on these two files
    /// 5. Stores the verification to the new challenge file
    /// 6. Verifies that the stored response hash in the new challenge file is correct
    /// 7. Construct the signed contribution data
    /// 8. Uploads the signature and new challenge file to the coordinator
    /// 9. Attempts to apply the verification in the ceremony
    ///     - Request to the coordinator to run `try_verify`
    ///
    pub async fn try_verify(&self) -> Result<(), VerifierError> {
        // Attempt to fetch a task from the queue or lock a chunk from the coordinator.
        let lock_response = match self.get_task().await {
            Ok(lock_response) => lock_response,
            Err(err) => {
                // If there are no tasks, attempt to join the queue for the next round.
                self.join_queue().await?;

                return Err(err);
            }
        };

        info!("Attempting to verify chunk {}", lock_response.chunk_id);

        // Deserialize the lock response.
        let LockResponse {
            chunk_id,
            locked: _,
            participant_id: _,
            challenge_locator,
            response_locator,
            next_challenge_locator,
        } = &lock_response;

        // Download and process the challenge file.
        let challenge_hash = self.process_challenge_file(&challenge_locator).await?;

        // Download and process the response file.
        let response_hash = self.process_response_file(&response_locator).await?;

        // Run verification on a chunk with the given locators.
        let _duration = self.run_verification(
            *chunk_id,
            &challenge_locator,
            &response_locator,
            &next_challenge_locator,
        );

        // Fetch the next challenge file from the filesystem.
        let (next_challenge_file, next_challenge_hash) = self.read_next_challenge_file(&next_challenge_locator).await?;

        // Verify that the next challenge file stores the correct response hash.
        self.verify_response_hash(&next_challenge_file, &response_hash)?;

        // Construct a signature and serialize the contribution.
        let signature_and_next_challenge_bytes = self.serialize_contribution_and_signature(
            challenge_hash,
            response_hash,
            next_challenge_hash,
            next_challenge_file,
        )?;

        // Upload the signature and new challenge file to `next_challenge_locator`.
        self.upload_next_challenge_locator_file(&next_challenge_locator, signature_and_next_challenge_bytes)
            .await?;
        // Attempt to perform the verification with the uploaded challenge file at `next_challenge_locator`.
        self.verify_contribution(*chunk_id).await?;

        // Clear the task from the cache.
        self.clear_task(&lock_response).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phase1_coordinator::environment::{Parameters, Testing};

    use rand::{Rng, SeedableRng};
    use rand_xorshift::XorShiftRng;
    use std::str::FromStr;

    const TEST_VIEW_KEY: &str = "AViewKey1cWNDyYMjc9p78PnCderRx37b9pJr4myQqmmPeCfeiLf3";

    pub fn test_verifier() -> Verifier {
        let environment: Testing = Testing::from(Parameters::TestCustom {
            number_of_chunks: 64,
            power: 16,
            batch_size: 512,
        });

        let view_key = ViewKey::from_str(TEST_VIEW_KEY).expect("Invalid view key");
        let address = Address::from_view_key(&view_key).expect("Address not derived correctly");

        Verifier::new(
            Url::from_str("http://test_coordinator_url").unwrap(),
            view_key,
            address,
            environment.into(),
            "TEST_VERIFIER.tasks".to_string(),
        )
        .unwrap()
    }

    #[test]
    pub fn test_verify_response_hash() {
        let mut rng = XorShiftRng::seed_from_u64(1231275789u64);

        let verifier = test_verifier();

        // Generate dummy contributions.
        let dummy_response: [u8; 32] = rng.gen();
        let dummy_challenge: [u8; 32] = rng.gen();

        let response_hash = calculate_hash(&dummy_response).to_vec();

        // Check that the invalid challenge fails to verify.
        assert!(verifier.verify_response_hash(&dummy_challenge, &response_hash).is_err());

        // Check that the challenge correctly stores the response hash.
        let challenge_with_stored_response_hash = [response_hash.to_vec(), dummy_challenge.to_vec()].concat();
        assert!(
            verifier
                .verify_response_hash(&challenge_with_stored_response_hash, &response_hash)
                .is_ok()
        );
    }

    #[test]
    pub fn test_contribution_signatures() {
        let mut rng = XorShiftRng::seed_from_u64(1231275789u64);

        let verifier = test_verifier();

        // Generate dummy contributions.
        let dummy_response: [u8; 32] = rng.gen();
        let dummy_challenge: [u8; 32] = rng.gen();
        let dummy_next_challenge: [u8; 32] = rng.gen();

        // Calculate contribution hashes.
        let response_hash = calculate_hash(&dummy_response).to_vec();
        let challenge_hash = calculate_hash(&dummy_challenge).to_vec();
        let next_challenge_hash = calculate_hash(&dummy_next_challenge).to_vec();

        // Construct the signature data
        let signed_contribution_data = verifier
            .sign_contribution_data(&challenge_hash, &response_hash, &next_challenge_hash)
            .unwrap();

        let signature = signed_contribution_data.get_signature();

        // Construct the signature message
        let contribution_state = ContributionState::new(
            challenge_hash.to_vec(),
            response_hash.to_vec(),
            Some(next_challenge_hash.to_vec()),
        )
        .unwrap();
        let message = contribution_state.signature_message().unwrap();

        // Derive the verifier address
        let address = Address::from_view_key(&verifier.view_key).unwrap();

        // Check that the signature verifies
        assert!(AleoAuthentication::verify(&address.to_string(), signature, message).unwrap())
    }

    #[test]
    pub fn test_contribution_serialization() {
        let mut rng = XorShiftRng::seed_from_u64(1231275789u64);

        let verifier = test_verifier();

        // Generate dummy contributions.
        let dummy_response: [u8; 32] = rng.gen();
        let dummy_challenge: [u8; 32] = rng.gen();
        let dummy_next_challenge: [u8; 32] = rng.gen();

        // Calculate contribution hashes.
        let response_hash = calculate_hash(&dummy_response).to_vec();
        let challenge_hash = calculate_hash(&dummy_challenge).to_vec();
        let next_challenge_hash = calculate_hash(&dummy_next_challenge).to_vec();

        // Construct the serialized contribution
        let mut serialized_contribution = verifier
            .serialize_contribution_and_signature(
                challenge_hash.to_vec(),
                response_hash.to_vec(),
                next_challenge_hash.to_vec(),
                dummy_next_challenge.to_vec(),
            )
            .unwrap();

        // The signature length is always 64 bytes
        let signature_length = 64;

        // Deserialize the contribution
        let declared_verifier_flag = serialized_contribution.drain(0..1).collect::<Vec<u8>>();
        let declared_signature = serialized_contribution.drain(0..signature_length).collect::<Vec<u8>>();
        let declared_challenge_hash = serialized_contribution
            .drain(0..challenge_hash.len())
            .collect::<Vec<u8>>();
        let declared_response_hash = serialized_contribution
            .drain(0..response_hash.len())
            .collect::<Vec<u8>>();
        let declared_next_challenge_hash = serialized_contribution
            .drain(0..next_challenge_hash.len())
            .collect::<Vec<u8>>();
        let declared_next_challenge_file = serialized_contribution
            .drain(0..dummy_next_challenge.len())
            .collect::<Vec<u8>>();

        assert_eq!(declared_verifier_flag, vec![1]);
        assert_eq!(declared_challenge_hash, challenge_hash);
        assert_eq!(declared_response_hash, response_hash);
        assert_eq!(declared_next_challenge_hash, next_challenge_hash);
        assert_eq!(declared_next_challenge_file, dummy_next_challenge);
    }
}
