use crate::{
    objects::Participant,
    storage::{ConcurrentMemory, InMemory, Storage},
};
use phase1::{helpers::CurveKind, ContributionMode, ProvingSystem};
use setup_utils::{CheckForCorrectness, UseCompression};

use rocket_cors::{AllowedHeaders, AllowedOrigins, Cors};
use url::Url;

type BatchSize = usize;
type ChunkSize = usize;
type Curve = CurveKind;
type NumberOfChunks = usize;
type Power = usize;
type CompressedInputs = UseCompression;
type CompressedOutputs = UseCompression;

pub type Settings = (
    ContributionMode,
    ProvingSystem,
    Curve,
    Power,
    BatchSize,
    ChunkSize,
    CompressedInputs,
    CompressedOutputs,
);

#[derive(Debug, Clone)]
pub enum Parameters {
    AleoInner,
    AleoOuter,
    AleoUniversal,
    AleoTest3Chunks,
    AleoTest3ChunksNoCompression,
    AleoTest8Chunks,
    AleoTest20Chunks,
    AleoTestChunks(NumberOfChunks),
    AleoTestCustom(NumberOfChunks, Power, BatchSize),
    Custom(Settings),
}

impl Parameters {
    /// Returns the corresponding settings for each parameter type.
    fn to_settings(&self) -> Settings {
        match self {
            Parameters::AleoInner => Self::aleo_inner(),
            Parameters::AleoOuter => Self::aleo_outer(),
            Parameters::AleoUniversal => Self::aleo_universal(),
            Parameters::AleoTest3Chunks => Self::aleo_test_3_chunks(),
            Parameters::AleoTest3ChunksNoCompression => Self::aleo_test_3_chunks_no_compression(),
            Parameters::AleoTest8Chunks => Self::aleo_test_8_chunks(),
            Parameters::AleoTest20Chunks => Self::aleo_test_20_chunks(),
            Parameters::AleoTestChunks(number_of_chunks) => Self::aleo_test_chunks(number_of_chunks),
            Parameters::AleoTestCustom(number_of_chunks, power, batch_size) => {
                Self::aleo_test_custom(number_of_chunks, power, batch_size)
            }
            Parameters::Custom(settings) => settings.clone(),
        }
    }

    fn aleo_inner() -> Settings {
        (
            ContributionMode::Chunked,
            ProvingSystem::Groth16,
            CurveKind::Bls12_377,
            Power::from(20_usize),
            BatchSize::from(64_usize),
            ChunkSize::from(2048_usize),
            UseCompression::No,
            UseCompression::Yes,
        )
    }

    fn aleo_outer() -> Settings {
        (
            ContributionMode::Chunked,
            ProvingSystem::Groth16,
            CurveKind::Bls12_377,
            Power::from(21_usize),
            BatchSize::from(64_usize),
            ChunkSize::from(8192_usize),
            UseCompression::No,
            UseCompression::Yes,
        )
    }

    fn aleo_universal() -> Settings {
        (
            ContributionMode::Chunked,
            ProvingSystem::Marlin,
            CurveKind::Bls12_377,
            Power::from(30_usize),
            BatchSize::from(64_usize),
            ChunkSize::from(8192_usize),
            UseCompression::No,
            UseCompression::Yes,
        )
    }

    fn aleo_test_3_chunks() -> Settings {
        (
            ContributionMode::Chunked,
            ProvingSystem::Groth16,
            CurveKind::Bls12_377,
            Power::from(8_usize),
            BatchSize::from(64_usize),
            ChunkSize::from(170_usize),
            UseCompression::No,
            UseCompression::Yes,
        )
    }

    fn aleo_test_3_chunks_no_compression() -> Settings {
        (
            ContributionMode::Chunked,
            ProvingSystem::Groth16,
            CurveKind::Bls12_377,
            Power::from(8_usize),
            BatchSize::from(64_usize),
            ChunkSize::from(170_usize),
            UseCompression::No,
            UseCompression::No,
        )
    }

    fn aleo_test_8_chunks() -> Settings {
        (
            ContributionMode::Chunked,
            ProvingSystem::Groth16,
            CurveKind::Bls12_377,
            Power::from(14_usize),
            BatchSize::from(64_usize),
            ChunkSize::from(4095_usize),
            UseCompression::No,
            UseCompression::Yes,
        )
    }

    fn aleo_test_20_chunks() -> Settings {
        (
            ContributionMode::Chunked,
            ProvingSystem::Groth16,
            CurveKind::Bls12_377,
            Power::from(14_usize),
            BatchSize::from(64_usize),
            ChunkSize::from(1638_usize),
            UseCompression::No,
            UseCompression::Yes,
        )
    }

    fn aleo_test_chunks(number_of_chunks: &NumberOfChunks) -> Settings {
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
            UseCompression::No,
            UseCompression::Yes,
        )
    }

    fn aleo_test_custom(number_of_chunks: &NumberOfChunks, power: &Power, batch_size: &BatchSize) -> Settings {
        let proving_system = ProvingSystem::Groth16;
        (
            ContributionMode::Chunked,
            proving_system,
            CurveKind::Bls12_377,
            *power,
            *batch_size,
            chunk_size!(number_of_chunks, proving_system, power),
            UseCompression::No,
            UseCompression::Yes,
        )
    }
}

#[derive(Debug, Clone)]
pub enum Environment {
    Test(Parameters),
    Development(Parameters),
    Production(Parameters),
}

impl Environment {
    /// Returns the parameter settings of the coordinator.
    pub fn to_settings(&self) -> Settings {
        match self {
            Environment::Test(parameters) => parameters.to_settings(),
            Environment::Development(parameters) => parameters.to_settings(),
            Environment::Production(parameters) => parameters.to_settings(),
        }
    }

    ///
    /// Returns the appropriate number of chunks for the coordinator
    /// to run given a proof system, power and chunk size.
    ///
    pub fn number_of_chunks(&self) -> u64 {
        let (_, proving_system, _, power, _, chunk_size, _, _) = match self {
            Environment::Test(parameters) => parameters.to_settings(),
            Environment::Development(parameters) => parameters.to_settings(),
            Environment::Production(parameters) => parameters.to_settings(),
        };
        total_size_in_g1!(proving_system, power) / chunk_size as u64
    }

    ///
    /// Returns the compressed input preference of the coordinator.
    ///
    /// By default, the coordinator returns `false` to minimize time
    /// spent by contributors on decompressing inputs.
    ///
    pub fn compressed_inputs(&self) -> UseCompression {
        let (_, _, _, _, _, _, compressed_inputs, _) = match self {
            Environment::Test(parameters) => parameters.to_settings(),
            Environment::Development(parameters) => parameters.to_settings(),
            Environment::Production(parameters) => parameters.to_settings(),
        };
        compressed_inputs
    }

    ///
    /// Returns the compressed output preference of the coordinator.
    ///
    /// By default, the coordinator returns `true` to minimize time
    /// spent by the coordinator and contributors on uploading chunks.
    ///
    pub fn compressed_outputs(&self) -> UseCompression {
        let (_, _, _, _, _, _, _, compressed_outputs) = match self {
            Environment::Test(parameters) => parameters.to_settings(),
            Environment::Development(parameters) => parameters.to_settings(),
            Environment::Production(parameters) => parameters.to_settings(),
        };
        compressed_outputs
    }

    ///
    /// Returns the input correctness check preference of the coordinator.
    ///
    /// By default, the coordinator returns `false` to minimize time
    /// spent by the contributors on reading chunks.
    ///
    pub fn check_input_for_correctness(&self) -> CheckForCorrectness {
        match self {
            Environment::Test(_) => CheckForCorrectness::No,
            Environment::Development(_) => CheckForCorrectness::No,
            Environment::Production(_) => CheckForCorrectness::No,
        }
    }

    /// Returns the base URL for the coordinator.
    pub fn base_url(&self) -> Url {
        format!("http://{}:{}", self.address(), self.port())
            .parse()
            .expect("Unable to parse base URL")
    }

    /// Returns the network address of the coordinator.
    pub const fn address(&self) -> &str {
        match self {
            Environment::Test(_) => "localhost",
            Environment::Development(_) => "0.0.0.0",
            Environment::Production(_) => "167.71.156.62",
        }
    }

    /// Returns the network port of the coordinator.
    pub const fn port(&self) -> u16 {
        match self {
            Environment::Test(_) => 8080,
            Environment::Development(_) => 8080,
            Environment::Production(_) => 8080,
        }
    }

    /// Returns the CORS policy of the server.
    pub fn cors(&self) -> Cors {
        let allowed_origins = match self {
            Environment::Test(_) => AllowedOrigins::all(),
            Environment::Development(_) => AllowedOrigins::all(),
            Environment::Production(_) => AllowedOrigins::all(),
        };

        let allowed_headers = match self {
            Environment::Test(_) => AllowedHeaders::all(),
            Environment::Development(_) => AllowedHeaders::all(),
            Environment::Production(_) => AllowedHeaders::all(),
        };

        Cors {
            allowed_origins,
            allowed_headers,
            allow_credentials: true,
            ..Default::default()
        }
    }

    /// Returns the base directory for the local locator of this coordinator.
    pub const fn local_base_directory(&self) -> &str {
        match self {
            Environment::Test(_) => "./transcript/test",
            Environment::Development(_) => "./transcript/development",
            Environment::Production(_) => "./transcript",
        }
    }

    /// Returns the round directory for a given round height.
    pub fn round_directory(&self, round_height: u64) -> String {
        round_directory!(self, Local, Remote, Remote, round_height)
    }

    /// Initializes the round directory for a given round height.
    pub fn round_directory_init(&self, round_height: u64) {
        round_directory_init!(self, Local, Remote, Remote, round_height)
    }

    /// Returns `true` if the round directory exists for a given round height.
    pub fn round_directory_exists(&self, round_height: u64) -> bool {
        round_directory_exists!(self, Local, Remote, Remote, round_height)
    }

    /// Resets the round directory for a given round height, if permitted.
    pub fn round_directory_reset(&self, round_height: u64) {
        round_directory_reset!(self, Local, Remote, Remote, round_height)
    }

    /// Resets the entire round directory for all rounds, if permitted.
    pub fn round_directory_reset_all(&self) {
        round_directory_reset_all!(self, Local, Remote, Remote)
    }

    /// Returns the chunk directory for a given round height and chunk ID.
    pub fn chunk_directory(&self, round_height: u64, chunk_id: u64) -> String {
        chunk_directory!(self, Local, Remote, Remote, round_height, chunk_id)
    }

    /// Initializes the chunk directory for a given round height and chunk ID.
    pub fn chunk_directory_init(&self, round_height: u64, chunk_id: u64) {
        chunk_directory_init!(self, Local, Remote, Remote, round_height, chunk_id)
    }

    /// Returns `true` if the chunk directory exists for a given round height and chunk ID.
    pub fn chunk_directory_exists(&self, round_height: u64, chunk_id: u64) -> bool {
        chunk_directory_exists!(self, Local, Remote, Remote, round_height, chunk_id)
    }

    /// Returns the contribution locator for a given round, chunk ID, and contribution ID.
    pub fn contribution_locator(
        &self,
        round_height: u64,
        chunk_id: u64,
        contribution_id: u64,
        verified: bool,
    ) -> String {
        contribution_locator!(
            self,
            Local,
            Remote,
            Remote,
            round_height,
            chunk_id,
            contribution_id,
            verified
        )
    }

    /// Initializes the contribution locator file for a given round, chunk ID, and contribution ID.
    pub fn contribution_locator_init(&self, round_height: u64, chunk_id: u64, contribution_id: u64) {
        contribution_locator_init!(self, Local, Remote, Remote, round_height, chunk_id, contribution_id)
    }

    /// Returns `true` if the contribution locator exists for a given round, chunk ID, and contribution ID.
    pub fn contribution_locator_exists(
        &self,
        round_height: u64,
        chunk_id: u64,
        contribution_id: u64,
        verified: bool,
    ) -> bool {
        contribution_locator_exists!(
            self,
            Local,
            Remote,
            Remote,
            round_height,
            chunk_id,
            contribution_id,
            verified
        )
    }

    /// Returns the round locator for a given round height.
    pub fn round_locator(&self, round_height: u64) -> String {
        round_locator!(self, Local, Remote, Remote, round_height)
    }

    /// Returns `true` if the round locator exists for a given round height.
    pub fn round_locator_exists(&self, round_height: u64) -> bool {
        round_locator_exists!(self, Local, Remote, Remote, round_height)
    }

    ///
    /// Returns the contributor managed by the coordinator.
    ///
    /// The primary purpose of this is to establish an identity for the coordinator
    /// when running initialization of each round.
    ///
    /// This can also be purposed for completing contributions of participants
    /// who may have dropped off and handed over control of their session.
    ///
    pub fn coordinator_contributor(&self) -> Participant {
        match self {
            Environment::Test(_) => Participant::Contributor(format!("test-coordinator-contributor")),
            Environment::Development(_) => Participant::Contributor(format!("development-coordinator-contributor")),
            Environment::Production(_) => Participant::Contributor(format!("production-coordinator-contributor")),
        }
    }

    /// Returns a verifier managed by the coordinator.
    pub fn coordinator_verifier(&self) -> Participant {
        match self {
            Environment::Test(_) => Participant::Verifier(format!("test-coordinator-verifier")),
            Environment::Development(_) => Participant::Verifier(format!("development-coordinator-verifier")),
            Environment::Production(_) => Participant::Verifier(format!("production-coordinator-verifier")),
        }
    }

    /// Returns the version number of the coordinator.
    pub const fn version(&self) -> u64 {
        match self {
            Environment::Test(_) => 1,
            Environment::Development(_) => 1,
            Environment::Production(_) => 1,
        }
    }

    /// Returns the storage system of the coordinator.
    pub(crate) fn storage(&self) -> anyhow::Result<Box<dyn Storage>> {
        Ok(storage!(self, InMemory, ConcurrentMemory, ConcurrentMemory))
    }
}

#[cfg(test)]
mod tests {
    use crate::environment::*;

    #[test]
    fn test_custom_chunk_3() {
        let number_of_chunks = 3;

        let parameters = Parameters::AleoTestChunks(number_of_chunks);
        let (_, _, _, power, _, chunk_size, _, _) = parameters.to_settings();
        assert_eq!(Power::from(14_usize), power);
        assert_eq!(ChunkSize::from(10922_usize), chunk_size);
        assert_eq!(
            number_of_chunks as u64,
            Environment::Test(parameters).number_of_chunks()
        );
    }

    #[test]
    fn test_custom_chunk_8() {
        let number_of_chunks = 8;

        let parameters = Parameters::AleoTestChunks(number_of_chunks);
        let (_, _, _, power, _, chunk_size, _, _) = parameters.to_settings();
        assert_eq!(Power::from(14_usize), power);
        assert_eq!(ChunkSize::from(4095_usize), chunk_size);
        assert_eq!(
            number_of_chunks as u64,
            Environment::Test(parameters).number_of_chunks()
        );
    }

    #[test]
    fn test_custom_chunk_20() {
        let number_of_chunks = 20;

        let parameters = Parameters::AleoTestChunks(number_of_chunks);
        let (_, _, _, power, _, chunk_size, _, _) = parameters.to_settings();
        assert_eq!(Power::from(14_usize), power);
        assert_eq!(ChunkSize::from(1638_usize), chunk_size);
        assert_eq!(
            number_of_chunks as u64,
            Environment::Test(parameters).number_of_chunks()
        );
    }
}
