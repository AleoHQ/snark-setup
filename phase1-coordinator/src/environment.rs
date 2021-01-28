use crate::{
    objects::Participant,
    serialize::string,
    storage::{Disk, Storage},
};
use phase1::{helpers::CurveKind, ContributionMode, ProvingSystem};
use setup_utils::{CheckForCorrectness, UseCompression};

use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use tracing::Level;
use url::Url;

/// Used in [Phase1Parameters::new_chunk()](phase1::objects::parameters::Phase1Parameters::new()).
/// Will cause a panic if set to 0.
type BatchSize = usize;
/// Used in [Phase1Parameters::new_chunk()](phase1::objects::parameters::Phase1Parameters::new()).
type ChunkSize = usize;
type Curve = CurveKind;
/// Used in [Phase1Parameters::new_chunk()](phase1::objects::parameters::Phase1Parameters::new()).
type NumberOfChunks = usize;
/// Used in [Phase1Parameters::new_chunk()](phase1::objects::parameters::Phase1Parameters::new()).
type Power = usize;

pub type Settings = (ContributionMode, ProvingSystem, Curve, Power, BatchSize, ChunkSize);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Deployment {
    Testing,
    Development,
    Production,
}

#[derive(Debug, Clone)]
pub enum Parameters {
    AleoInner,
    AleoOuter,
    AleoUniversal,
    Custom(Settings),
    Test3Chunks,
    Test8Chunks,
    TestChunks(NumberOfChunks),
    TestCustom(NumberOfChunks, Power, BatchSize),
}

impl Parameters {
    /// Returns the corresponding settings for each parameter type.
    fn to_settings(&self) -> Settings {
        match self {
            Parameters::AleoInner => Self::aleo_inner(),
            Parameters::AleoOuter => Self::aleo_outer(),
            Parameters::AleoUniversal => Self::aleo_universal(),
            Parameters::Custom(settings) => *settings,
            Parameters::Test3Chunks => Self::test_3_chunks(),
            Parameters::Test8Chunks => Self::test_8_chunks(),
            Parameters::TestChunks(number_of_chunks) => Self::test_chunks(number_of_chunks),
            Parameters::TestCustom(number_of_chunks, power, batch_size) => {
                Self::test_custom(number_of_chunks, power, batch_size)
            }
        }
    }

    fn aleo_inner() -> Settings {
        (
            ContributionMode::Chunked,
            ProvingSystem::Groth16,
            CurveKind::Bls12_377,
            Power::from(20_usize),
            BatchSize::from(4096_usize),
            ChunkSize::from(32768_usize),
        )
    }

    fn aleo_outer() -> Settings {
        (
            ContributionMode::Chunked,
            ProvingSystem::Groth16,
            CurveKind::BW6,
            Power::from(21_usize),
            BatchSize::from(4096_usize),
            ChunkSize::from(65536_usize),
        )
    }

    fn aleo_universal() -> Settings {
        (
            ContributionMode::Chunked,
            ProvingSystem::Marlin,
            CurveKind::Bls12_377,
            Power::from(28_usize),
            BatchSize::from(4096_usize),
            ChunkSize::from(65536_usize),
        )
    }

    fn test_3_chunks() -> Settings {
        (
            ContributionMode::Chunked,
            ProvingSystem::Groth16,
            CurveKind::Bls12_377,
            Power::from(8_usize),
            BatchSize::from(64_usize),
            ChunkSize::from(172_usize),
        )
    }

    fn test_8_chunks() -> Settings {
        (
            ContributionMode::Chunked,
            ProvingSystem::Groth16,
            CurveKind::Bls12_377,
            Power::from(14_usize),
            BatchSize::from(64_usize),
            ChunkSize::from(4096_usize),
        )
    }

    fn test_chunks(number_of_chunks: &NumberOfChunks) -> Settings {
        let proving_system = ProvingSystem::Groth16;
        let power = 14_usize;
        let batch_size = 128_usize;
        (
            ContributionMode::Chunked,
            proving_system,
            CurveKind::Bls12_377,
            Power::from(power),
            BatchSize::from(batch_size),
            chunk_size!(number_of_chunks, proving_system, power),
        )
    }

    fn test_custom(number_of_chunks: &NumberOfChunks, power: &Power, batch_size: &BatchSize) -> Settings {
        let proving_system = ProvingSystem::Groth16;
        (
            ContributionMode::Chunked,
            proving_system,
            CurveKind::Bls12_377,
            *power,
            *batch_size,
            chunk_size!(number_of_chunks, proving_system, power),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    /// The parameter settings of this coordinator.
    parameters: Settings,
    /// The compressed input setting of the coordinator.
    compressed_inputs: UseCompression,
    /// The compressed output setting of the coordinator.
    compressed_outputs: UseCompression,
    /// The input correctness check preference of the coordinator.
    check_input_for_correctness: CheckForCorrectness,

    /// The minimum number of contributors permitted to participate in a round.
    minimum_contributors_per_round: usize,
    /// The maximum number of contributors permitted to participate in a round.
    maximum_contributors_per_round: usize,
    /// The minimum number of verifiers permitted to participate in a round.
    minimum_verifiers_per_round: usize,
    /// The maximum number of verifiers permitted to participate in a round.
    maximum_verifiers_per_round: usize,
    /// The number of chunks a contributor is authorized to lock in tandem in a round.
    contributor_lock_chunk_limit: usize,
    /// The number of chunks a verifier is authorized to lock in tandem in a round.
    verifier_lock_chunk_limit: usize,
    /// The number of minutes tolerated prior to assuming a contributor has dropped.
    contributor_timeout_in_minutes: u16,
    /// The number of minutes tolerated prior to assuming a verifier has dropped.
    verifier_timeout_in_minutes: u16,
    /// The number of drops tolerated by a participant before banning them from future rounds.
    participant_ban_threshold: u16,
    /// The setting to allow current contributors to join the queue for the next round.
    allow_current_contributors_in_queue: bool,
    /// The setting to allow current verifiers to join the queue for the next round.
    allow_current_verifiers_in_queue: bool,
    /// The minimum number of seconds to wait after aggregation before starting the next round.
    queue_wait_time: u64,

    /// The contributors managed by the coordinator.
    coordinator_contributors: Vec<Participant>,
    /// The verifiers managed by the coordinator.
    coordinator_verifiers: Vec<Participant>,

    /// The software version number of the coordinator.
    software_version: u64,
    /// The deployment environment of this coordinator.
    deployment: Deployment,
    /// The base directory for disk storage of this coordinator.
    local_base_directory: String,
    /// The logging verbosity of this coordinator.
    #[serde(with = "string")]
    verbosity: Level,
    /// The network address for the coordinator.
    address: String,
    /// The network port for the coordinator.
    port: u16,
}

impl Environment {
    ///
    /// Returns the parameter settings of the coordinator.
    ///
    pub fn parameters(&self) -> Settings {
        self.parameters
    }

    ///
    /// Returns the compressed input setting of the coordinator.
    ///
    /// The default choice should be `UseCompression::No` to minimize time
    /// spent by contributors on decompressing inputs.
    ///
    pub const fn compressed_inputs(&self) -> UseCompression {
        self.compressed_inputs
    }

    ///
    /// Returns the compressed output setting of the coordinator.
    ///
    /// The default choice should be `UseCompression::Yes` to minimize time
    /// spent by the coordinator and contributors on uploading chunks.
    ///
    pub const fn compressed_outputs(&self) -> UseCompression {
        self.compressed_outputs
    }

    ///
    /// Returns the input correctness check preference of the coordinator.
    ///
    /// The default choice should be `CheckForCorrectness::No` to minimize time
    /// spent by the contributors on reading chunks.
    ///
    pub fn check_input_for_correctness(&self) -> CheckForCorrectness {
        self.check_input_for_correctness
    }

    ///
    /// Returns the minimum number of contributors permitted to
    /// participate in a round.
    ///
    pub const fn minimum_contributors_per_round(&self) -> usize {
        self.minimum_contributors_per_round
    }

    ///
    /// Returns the maximum number of contributors permitted to
    /// participate in a round.
    ///
    pub const fn maximum_contributors_per_round(&self) -> usize {
        self.maximum_contributors_per_round
    }

    ///
    /// Returns the minimum number of verifiers permitted to
    /// participate in a round.
    ///
    pub const fn minimum_verifiers_per_round(&self) -> usize {
        self.minimum_verifiers_per_round
    }

    ///
    /// Returns the maximum number of verifiers permitted to
    /// participate in a round.
    ///
    pub const fn maximum_verifiers_per_round(&self) -> usize {
        self.maximum_verifiers_per_round
    }

    ///
    /// Returns the number of chunks a contributor is
    /// authorized to lock in tandem at any point during a round.
    ///
    pub const fn contributor_lock_chunk_limit(&self) -> usize {
        self.contributor_lock_chunk_limit
    }

    ///
    /// Returns the number of chunks a verifier is
    /// authorized to lock in tandem at any point during a round.
    ///
    pub const fn verifier_lock_chunk_limit(&self) -> usize {
        self.verifier_lock_chunk_limit
    }

    ///
    /// Returns the number of minutes the coordinator tolerates
    /// before assuming a contributor has disconnected.
    ///
    pub const fn contributor_timeout_in_minutes(&self) -> u16 {
        self.contributor_timeout_in_minutes
    }

    ///
    /// Returns the number of minutes the coordinator tolerates
    /// before assuming a verifier has disconnected.
    ///
    pub const fn verifier_timeout_in_minutes(&self) -> u16 {
        self.verifier_timeout_in_minutes
    }

    ///
    /// Returns the number of times the coordinator tolerates
    /// a dropped participant before banning them from future rounds.
    ///
    pub const fn participant_ban_threshold(&self) -> u16 {
        self.participant_ban_threshold
    }

    ///
    /// Returns the setting to allow current contributors to
    /// join the queue for the next round.
    ///
    pub const fn allow_current_contributors_in_queue(&self) -> bool {
        self.allow_current_contributors_in_queue
    }

    ///
    /// Returns the setting to allow current verifiers to
    /// join the queue for the next round.
    ///
    pub const fn allow_current_verifiers_in_queue(&self) -> bool {
        self.allow_current_verifiers_in_queue
    }

    ///
    /// Returns the minimum number of seconds to wait after aggregation
    /// before starting the next round.
    ///
    pub const fn queue_wait_time(&self) -> u64 {
        self.queue_wait_time
    }

    ///
    /// Returns the contributors managed by the coordinator.
    ///
    /// The primary purpose of this is to establish an identity for the coordinator
    /// when running initialization of each round.
    ///
    /// This can also be purposed for completing contributions of participants
    /// who may have dropped off and handed over control of their session.
    ///
    pub const fn coordinator_contributors(&self) -> &Vec<Participant> {
        &self.coordinator_contributors
    }

    /// Returns the verifiers managed by the coordinator.
    pub const fn coordinator_verifiers(&self) -> &Vec<Participant> {
        &self.coordinator_verifiers
    }

    ///
    /// Returns the software version number of the coordinator.
    ///
    pub const fn software_version(&self) -> u64 {
        self.software_version
    }

    ///
    /// Returns the deployment environment of the coordinator.
    ///
    pub const fn deployment(&self) -> &Deployment {
        &self.deployment
    }

    ///
    /// Returns the base directory for disk storage of this coordinator.
    ///
    pub fn local_base_directory(&self) -> &str {
        &self.local_base_directory
    }

    ///
    /// Returns the logging verbosity of this coordinator.
    ///
    pub fn verbosity(&self) -> &tracing::Level {
        &self.verbosity
    }

    ///
    /// Returns the network address of the coordinator.
    ///
    pub fn address(&self) -> &str {
        &self.address
    }

    ///
    /// Returns the network port of the coordinator.
    ///
    pub const fn port(&self) -> u16 {
        self.port
    }

    ///
    /// Returns the base URL for the coordinator.
    ///
    pub fn base_url(&self) -> Url {
        format!("http://{}:{}", self.address, self.port).parse().unwrap()
    }

    ///
    /// Returns the appropriate number of chunks for the coordinator
    /// to run given a proof system, power and chunk size.
    ///
    pub fn number_of_chunks(&self) -> u64 {
        let (_, proving_system, _, power, _, chunk_size) = self.parameters;
        (total_size_in_g1!(proving_system, power) + chunk_size as u64 - 1) / chunk_size as u64
    }

    /// Returns the storage system of the coordinator.
    pub(crate) fn storage(&self) -> anyhow::Result<Box<dyn Storage>> {
        Ok(Box::new(Disk::load(self)?))
    }
}

impl From<Testing> for Environment {
    fn from(deployment: Testing) -> Environment {
        deployment.environment
    }
}

impl From<Development> for Environment {
    fn from(deployment: Development) -> Environment {
        deployment.environment
    }
}

impl From<Production> for Environment {
    fn from(deployment: Production) -> Environment {
        deployment.environment
    }
}

// TODO (howardwu): Convert the implementation to a procedural macro.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Testing {
    pub environment: Environment,
}

impl Testing {
    #[inline]
    pub fn minimum_contributors_per_round(&self, minimum: usize) -> Self {
        let mut deployment = self.clone();
        deployment.environment.minimum_contributors_per_round = minimum;
        deployment
    }

    #[inline]
    pub fn maximum_contributors_per_round(&self, maximum: usize) -> Self {
        let mut deployment = self.clone();
        deployment.environment.maximum_contributors_per_round = maximum;
        deployment
    }

    #[inline]
    pub fn coordinator_contributors(&self, contributors: &[Participant]) -> Self {
        // Check that all participants are contributors.
        if contributors.into_par_iter().filter(|p| !p.is_contributor()).count() > 0 {
            panic!("Specifying to environment a list of coordinator contributors with non-contributors.")
        }

        let mut deployment = self.clone();
        deployment.environment.coordinator_contributors = contributors.to_vec();
        deployment
    }

    #[inline]
    pub fn coordinator_verifiers(&self, verifiers: &[Participant]) -> Self {
        // Check that all participants are verifiers.
        if verifiers.into_par_iter().filter(|p| !p.is_verifier()).count() > 0 {
            panic!("Specifying to environment a list of coordinator verifiers with non-verifiers.")
        }

        let mut deployment = self.clone();
        deployment.environment.coordinator_verifiers = verifiers.to_vec();
        deployment
    }

    #[inline]
    pub fn verbosity(&self, verbosity: &str) -> Self {
        let mut deployment = self.clone();
        deployment.environment.verbosity = match verbosity {
            "ERROR" => Level::ERROR,
            "WARN" => Level::WARN,
            "INFO" => Level::INFO,
            "DEBUG" => Level::DEBUG,
            "TRACE" => Level::TRACE,
            _ => Level::TRACE,
        };
        deployment
    }
}

impl From<Parameters> for Testing {
    fn from(parameters: Parameters) -> Self {
        let mut testing = Self::default();
        testing.environment.parameters = parameters.to_settings();
        testing
    }
}

impl std::ops::Deref for Testing {
    type Target = Environment;

    fn deref(&self) -> &Self::Target {
        &self.environment
    }
}

impl std::default::Default for Testing {
    fn default() -> Self {
        Self {
            environment: Environment {
                parameters: Parameters::Test3Chunks.to_settings(),
                compressed_inputs: UseCompression::No,
                compressed_outputs: UseCompression::Yes,
                check_input_for_correctness: CheckForCorrectness::No,

                minimum_contributors_per_round: 1,
                maximum_contributors_per_round: 5,
                minimum_verifiers_per_round: 1,
                maximum_verifiers_per_round: 5,
                contributor_lock_chunk_limit: 5,
                verifier_lock_chunk_limit: 5,
                contributor_timeout_in_minutes: 5,
                verifier_timeout_in_minutes: 15,
                participant_ban_threshold: 5,
                allow_current_contributors_in_queue: true,
                allow_current_verifiers_in_queue: true,
                queue_wait_time: 0,

                coordinator_contributors: vec![Participant::new_contributor("testing-coordinator-contributor")],
                coordinator_verifiers: vec![Participant::new_verifier("testing-coordinator-verifier")],

                software_version: 1,
                deployment: Deployment::Testing,
                local_base_directory: "./transcript/testing".to_string(),
                verbosity: Level::TRACE,
                address: "localhost".to_string(),
                port: 8080,
            },
        }
    }
}

// TODO (howardwu): Convert the implementation to a procedural macro.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Development {
    environment: Environment,
}

impl Development {
    #[inline]
    pub fn minimum_contributors_per_round(&self, minimum: usize) -> Self {
        let mut deployment = self.clone();
        deployment.environment.minimum_contributors_per_round = minimum;
        deployment
    }

    #[inline]
    pub fn maximum_contributors_per_round(&self, maximum: usize) -> Self {
        let mut deployment = self.clone();
        deployment.environment.maximum_contributors_per_round = maximum;
        deployment
    }

    #[inline]
    pub fn coordinator_contributors(&self, contributors: &[Participant]) -> Self {
        // Check that all participants are contributors.
        if contributors.into_par_iter().filter(|p| !p.is_contributor()).count() > 0 {
            panic!("Specifying to environment a list of coordinator contributors with non-contributors.")
        }

        let mut deployment = self.clone();
        deployment.environment.coordinator_contributors = contributors.to_vec();
        deployment
    }

    #[inline]
    pub fn coordinator_verifiers(&self, verifiers: &[Participant]) -> Self {
        // Check that all participants are verifiers.
        if verifiers.into_par_iter().filter(|p| !p.is_verifier()).count() > 0 {
            panic!("Specifying to environment a list of coordinator verifiers with non-verifiers.")
        }

        let mut deployment = self.clone();
        deployment.environment.coordinator_verifiers = verifiers.to_vec();
        deployment
    }

    #[inline]
    pub fn verbosity(&self, verbosity: &str) -> Self {
        let mut deployment = self.clone();
        deployment.environment.verbosity = match verbosity {
            "ERROR" => Level::ERROR,
            "WARN" => Level::WARN,
            "INFO" => Level::INFO,
            "DEBUG" => Level::DEBUG,
            "TRACE" => Level::TRACE,
            _ => Level::DEBUG,
        };
        deployment
    }
}

impl From<Parameters> for Development {
    fn from(parameters: Parameters) -> Self {
        let mut testing = Self::default();
        testing.environment.parameters = parameters.to_settings();
        testing
    }
}

impl std::ops::Deref for Development {
    type Target = Environment;

    fn deref(&self) -> &Self::Target {
        &self.environment
    }
}

impl std::ops::DerefMut for Development {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.environment
    }
}

impl std::default::Default for Development {
    fn default() -> Self {
        Self {
            environment: Environment {
                parameters: Parameters::AleoInner.to_settings(),
                compressed_inputs: UseCompression::No,
                compressed_outputs: UseCompression::Yes,
                check_input_for_correctness: CheckForCorrectness::No,

                minimum_contributors_per_round: 1,
                maximum_contributors_per_round: 5,
                minimum_verifiers_per_round: 1,
                maximum_verifiers_per_round: 5,
                contributor_lock_chunk_limit: 5,
                verifier_lock_chunk_limit: 5,
                contributor_timeout_in_minutes: 5,
                verifier_timeout_in_minutes: 15,
                participant_ban_threshold: 5,
                allow_current_contributors_in_queue: true,
                allow_current_verifiers_in_queue: true,
                queue_wait_time: 60,

                coordinator_contributors: vec![Participant::new_contributor("development-coordinator-contributor")],
                coordinator_verifiers: vec![Participant::new_verifier("development-coordinator-verifier")],

                software_version: 1,
                deployment: Deployment::Development,
                local_base_directory: "./transcript/development".to_string(),
                verbosity: Level::DEBUG,
                address: "0.0.0.0".to_string(),
                port: 8080,
            },
        }
    }
}

// TODO (howardwu): Convert the implementation to a procedural macro.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Production {
    environment: Environment,
}

impl Production {
    #[inline]
    pub fn minimum_contributors_per_round(&self, minimum: usize) -> Self {
        let mut deployment = self.clone();
        deployment.environment.minimum_contributors_per_round = minimum;
        deployment
    }

    #[inline]
    pub fn maximum_contributors_per_round(&self, maximum: usize) -> Self {
        let mut deployment = self.clone();
        deployment.environment.maximum_contributors_per_round = maximum;
        deployment
    }

    #[inline]
    pub fn coordinator_contributors(&self, contributors: &[Participant]) -> Self {
        // Check that all participants are contributors.
        if contributors.into_par_iter().filter(|p| !p.is_contributor()).count() > 0 {
            panic!("Specifying to environment a list of coordinator contributors with non-contributors.")
        }

        let mut deployment = self.clone();
        deployment.environment.coordinator_contributors = contributors.to_vec();
        deployment
    }

    #[inline]
    pub fn coordinator_verifiers(&self, verifiers: &[Participant]) -> Self {
        // Check that all participants are verifiers.
        if verifiers.into_par_iter().filter(|p| !p.is_verifier()).count() > 0 {
            panic!("Specifying to environment a list of coordinator verifiers with non-verifiers.")
        }

        let mut deployment = self.clone();
        deployment.environment.coordinator_verifiers = verifiers.to_vec();
        deployment
    }

    #[inline]
    pub fn verbosity(&self, verbosity: &str) -> Self {
        let mut deployment = self.clone();
        deployment.environment.verbosity = match verbosity {
            "ERROR" => Level::ERROR,
            "WARN" => Level::WARN,
            "INFO" => Level::INFO,
            "DEBUG" => Level::DEBUG,
            "TRACE" => Level::TRACE,
            _ => Level::DEBUG,
        };
        deployment
    }
}

impl From<Parameters> for Production {
    fn from(parameters: Parameters) -> Self {
        let mut testing = Self::default();
        testing.environment.parameters = parameters.to_settings();
        testing
    }
}

impl std::ops::Deref for Production {
    type Target = Environment;

    fn deref(&self) -> &Self::Target {
        &self.environment
    }
}

impl std::default::Default for Production {
    fn default() -> Self {
        Self {
            environment: Environment {
                parameters: Parameters::AleoInner.to_settings(),
                compressed_inputs: UseCompression::No,
                compressed_outputs: UseCompression::Yes,
                check_input_for_correctness: CheckForCorrectness::No,

                minimum_contributors_per_round: 1,
                maximum_contributors_per_round: 2,
                minimum_verifiers_per_round: 1,
                maximum_verifiers_per_round: 5,
                contributor_lock_chunk_limit: 5,
                verifier_lock_chunk_limit: 5,
                contributor_timeout_in_minutes: 5,
                verifier_timeout_in_minutes: 15,
                participant_ban_threshold: 5,
                allow_current_contributors_in_queue: false,
                allow_current_verifiers_in_queue: true,
                queue_wait_time: 120,

                coordinator_contributors: vec![Participant::new_contributor("coordinator-contributor")],
                coordinator_verifiers: vec![Participant::new_verifier("coordinator-verifier")],

                software_version: 1,
                deployment: Deployment::Production,
                local_base_directory: "./transcript".to_string(),
                verbosity: Level::DEBUG,
                address: "0.0.0.0".to_string(),
                port: 443,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::environment::*;

    #[test]
    fn test_aleo_test_3_chunks() {
        let parameters = Parameters::Test3Chunks;
        let (_, _, _, power, _, _) = parameters.to_settings();
        assert_eq!(Power::from(8_usize), power);
        assert_eq!(3, Testing::from(parameters).number_of_chunks());
    }

    #[test]
    fn test_aleo_test_8_chunks() {
        let parameters = Parameters::Test8Chunks;
        let (_, _, _, power, _, _) = parameters.to_settings();
        assert_eq!(Power::from(14_usize), power);
        assert_eq!(8, Testing::from(parameters).number_of_chunks());
    }

    #[test]
    fn test_custom_chunk_3() {
        let number_of_chunks = 3;

        let parameters = Parameters::TestChunks(number_of_chunks);
        let (_, _, _, power, _, chunk_size) = parameters.to_settings();
        assert_eq!(Power::from(14_usize), power);
        assert_eq!(ChunkSize::from(10923_usize), chunk_size);
        assert_eq!(number_of_chunks as u64, Testing::from(parameters).number_of_chunks());
    }

    #[test]
    fn test_custom_chunk_8() {
        let number_of_chunks = 8;

        let parameters = Parameters::TestChunks(number_of_chunks);
        let (_, _, _, power, _, chunk_size) = parameters.to_settings();
        assert_eq!(Power::from(14_usize), power);
        assert_eq!(ChunkSize::from(4096_usize), chunk_size);
        assert_eq!(number_of_chunks as u64, Testing::from(parameters).number_of_chunks());
    }

    #[test]
    fn test_custom_chunk_20() {
        let number_of_chunks = 20;

        let parameters = Parameters::TestChunks(number_of_chunks);
        let (_, _, _, power, _, chunk_size) = parameters.to_settings();
        assert_eq!(Power::from(14_usize), power);
        assert_eq!(ChunkSize::from(1639_usize), chunk_size);
        assert_eq!(number_of_chunks as u64, Testing::from(parameters).number_of_chunks());
    }
}
