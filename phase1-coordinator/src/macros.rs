/// Returns an instance for chunked `Phase1Parameters` given an instantiation of `PairingEngine`,
/// an instance of `Settings`, and a chunk ID.
#[macro_export]
macro_rules! phase1_chunked_parameters {
    ($curve:ident, $settings:ident, $chunk_id:ident) => {{
        use phase1::Phase1Parameters;

        let (contribution_mode, proving_system, _, power, batch_size, chunk_size, _, _) = $settings;
        Phase1Parameters::<$curve>::new_chunk(
            contribution_mode,
            $chunk_id as usize,
            chunk_size,
            proving_system,
            power,
            batch_size,
        )
    }};
}

/// Returns an instance for full `Phase1Parameters` given an instantiation of `PairingEngine`,
/// an instance of `Settings`.
#[macro_export]
macro_rules! phase1_full_parameters {
    ($curve:ident, $settings:ident) => {{
        use phase1::Phase1Parameters;

        let (_, proving_system, _, power, batch_size, _, _, _) = $settings;
        Phase1Parameters::<$curve>::new_full(proving_system, power, batch_size)
    }};
}

/// Returns the contribution filesize given an instantiation of `PairingEngine`,
/// an instance of `Settings`, a chunk ID, a compressed setting, and whether
/// this is the initialization round.
#[macro_export]
macro_rules! contribution_filesize {
    ($curve:ident, $settings:ident, $chunk_id:ident, $compressed:ident) => {{
        use setup_utils::UseCompression;

        let parameters = phase1_chunked_parameters!($curve, $settings, $chunk_id);
        match $compressed {
            UseCompression::Yes => parameters.contribution_size as u64,
            UseCompression::No => (parameters.accumulator_size + parameters.public_key_size) as u64,
        }
    }};
}

/// Returns the total number of powers of tau G1 given a proving system and the number of powers.
#[macro_export]
macro_rules! total_size_in_g1 {
    ($proving_system:ident, $power:ident) => {{
        use phase1::ProvingSystem;

        match $proving_system {
            ProvingSystem::Groth16 => ((1 << ($power + 1)) - 1),
            ProvingSystem::Marlin => (1 << $power),
        }
    }};
}

/// Returns the chunk size given the desired number of chunks, the proving system,
/// and the number of powers.
#[macro_export]
macro_rules! chunk_size {
    ($num_chunks:ident, $proving_system:ident, $power:ident) => {{ (total_size_in_g1!($proving_system, $power) / $num_chunks) }};
}

/// Returns the final round filesize given an instantiation of `PairingEngine`,
/// an instance of `Settings`, a chunk ID, a compressed setting, and whether
/// this is the initialization round.
#[macro_export]
macro_rules! round_filesize {
    ($curve:ident, $settings:ident, $chunk_id:ident, $compressed:ident, $init:ident) => {{
        use phase1::Phase1Parameters;

        let full_parameters = phase1_full_parameters!($curve, $settings);
        let parameters = Phase1Parameters::<$curve>::new(
            full_parameters.contribution_mode,
            0,
            full_parameters.powers_g1_length, // <- do not change this
            full_parameters.curve.clone(),
            full_parameters.proving_system,
            full_parameters.total_size_in_log2,
            full_parameters.batch_size,
        );
        match ($compressed, $init) {
            (UseCompression::Yes, true) => (parameters.contribution_size - parameters.public_key_size) as u64,
            (UseCompression::Yes, false) => parameters.contribution_size as u64,
            (UseCompression::No, _) => parameters.accumulator_size as u64,
        }
    }};
}

/// Returns an instance of storage based on the environment the coordinator is operating in.
#[macro_export]
macro_rules! storage {
    ($env:ident, $l1:ident, $l2:ident, $l3:ident) => {{
        match $env {
            Environment::Test(_) => Box::new($l1::load($env)?),
            Environment::Development(_) => Box::new($l2::load($env)?),
            Environment::Production(_) => Box::new($l3::load($env)?),
        }
    }};
}

/// Returns the round directory using a locator that is determined based
/// on the environment the coordinator is operating in.
#[macro_export]
macro_rules! round_directory {
    ($env:ident, $l1:ident, $l2:ident, $l3:ident, $round_height:ident) => {{
        use crate::locators::*;

        match $env {
            Environment::Test(_) => $l1::round_directory($env, $round_height),
            Environment::Development(_) => $l2::round_directory($env, $round_height),
            Environment::Production(_) => $l3::round_directory($env, $round_height),
        }
    }};
}

/// Initializes the round directory for a given round height using a locator that is
/// determined based on the environment the coordinator is operating in.
#[macro_export]
macro_rules! round_directory_init {
    ($env:ident, $l1:ident, $l2:ident, $l3:ident, $round_height:ident) => {{
        use crate::locators::*;

        match $env {
            Environment::Test(_) => $l1::round_directory_init($env, $round_height),
            Environment::Development(_) => $l2::round_directory_init($env, $round_height),
            Environment::Production(_) => $l3::round_directory_init($env, $round_height),
        }
    }};
}

/// Returns `true` if the round directory exists using a locator that is determined based
/// on the environment the coordinator is operating in.
#[macro_export]
macro_rules! round_directory_exists {
    ($env:ident, $l1:ident, $l2:ident, $l3:ident, $round_height:ident) => {{
        use crate::locators::*;

        match $env {
            Environment::Test(_) => $l1::round_directory_exists($env, $round_height),
            Environment::Development(_) => $l2::round_directory_exists($env, $round_height),
            Environment::Production(_) => $l3::round_directory_exists($env, $round_height),
        }
    }};
}

/// Resets the round directory for a given round height if permitted using a locator
/// that is determined based on the environment the coordinator is operating in.
#[macro_export]
macro_rules! round_directory_reset {
    ($env:ident, $l1:ident, $l2:ident, $l3:ident, $round_height:ident) => {{
        use crate::locators::*;

        match $env {
            Environment::Test(_) => $l1::round_directory_reset($env, $round_height),
            Environment::Development(_) => $l2::round_directory_reset($env, $round_height),
            Environment::Production(_) => $l3::round_directory_reset($env, $round_height),
        }
    }};
}

/// Resets the entire round directory if permitted using a locator that is determined based
/// on the environment the coordinator is operating in.
#[macro_export]
macro_rules! round_directory_reset_all {
    ($env:ident, $l1:ident, $l2:ident, $l3:ident) => {{
        use crate::locators::*;

        match $env {
            Environment::Test(_) => $l1::round_directory_reset_all($env),
            Environment::Development(_) => $l2::round_directory_reset_all($env),
            Environment::Production(_) => $l3::round_directory_reset_all($env),
        }
    }};
}

/// Returns the chunk directory using a locator that is determined based
/// on the environment the coordinator is operating in.
#[macro_export]
macro_rules! chunk_directory {
    ($env:ident, $l1:ident, $l2:ident, $l3:ident, $round_height:ident, $chunk_id:ident) => {{
        use crate::locators::*;

        match $env {
            Environment::Test(_) => $l1::chunk_directory($env, $round_height, $chunk_id),
            Environment::Development(_) => $l2::chunk_directory($env, $round_height, $chunk_id),
            Environment::Production(_) => $l3::chunk_directory($env, $round_height, $chunk_id),
        }
    }};
}

/// Initializes the chunk directory for a given round height and chunk ID using a locator
/// that is determined based on the environment the coordinator is operating in.
#[macro_export]
macro_rules! chunk_directory_init {
    ($env:ident, $l1:ident, $l2:ident, $l3:ident, $round_height:ident, $chunk_id:ident) => {{
        use crate::locators::*;

        match $env {
            Environment::Test(_) => $l1::chunk_directory_init($env, $round_height, $chunk_id),
            Environment::Development(_) => $l2::chunk_directory_init($env, $round_height, $chunk_id),
            Environment::Production(_) => $l3::chunk_directory_init($env, $round_height, $chunk_id),
        }
    }};
}

/// Returns `true` if the chunk directory exists using a locator that is determined based
/// on the environment the coordinator is operating in.
#[macro_export]
macro_rules! chunk_directory_exists {
    ($env:ident, $l1:ident, $l2:ident, $l3:ident, $round_height:ident, $chunk_id:ident) => {{
        use crate::locators::*;

        match $env {
            Environment::Test(_) => $l1::chunk_directory_exists($env, $round_height, $chunk_id),
            Environment::Development(_) => $l2::chunk_directory_exists($env, $round_height, $chunk_id),
            Environment::Production(_) => $l3::chunk_directory_exists($env, $round_height, $chunk_id),
        }
    }};
}

/// Returns the contribution locator using a locator that is determined based
/// on the environment the coordinator is operating in.
#[macro_export]
macro_rules! contribution_locator {
    ($env:ident, $l1:ident, $l2:ident, $l3:ident, $round_height:ident, $chunk_id:ident, $cont_id:ident, $verified:ident) => {{
        use crate::locators::*;

        match $env {
            Environment::Test(_) => $l1::contribution_locator($env, $round_height, $chunk_id, $cont_id, $verified),
            Environment::Development(_) => {
                $l2::contribution_locator($env, $round_height, $chunk_id, $cont_id, $verified)
            }
            Environment::Production(_) => {
                $l3::contribution_locator($env, $round_height, $chunk_id, $cont_id, $verified)
            }
        }
    }};
}

/// Initializes the contribution locator using a locator that is determined based
/// on the environment the coordinator is operating in.
#[macro_export]
macro_rules! contribution_locator_init {
    ($env:ident, $l1:ident, $l2:ident, $l3:ident, $round_height:ident, $chunk_id:ident, $cont_id:ident) => {{
        use crate::locators::*;

        match $env {
            Environment::Test(_) => $l1::contribution_locator_init($env, $round_height, $chunk_id, $cont_id),
            Environment::Development(_) => $l2::contribution_locator_init($env, $round_height, $chunk_id, $cont_id),
            Environment::Production(_) => $l3::contribution_locator_init($env, $round_height, $chunk_id, $cont_id),
        }
    }};
}

/// Returns `true` if the contribution locator exists using a locator that is determined based
/// on the environment the coordinator is operating in.
#[macro_export]
macro_rules! contribution_locator_exists {
    ($env:ident, $l1:ident, $l2:ident, $l3:ident, $round_height:ident, $chunk_id:ident, $cont_id:ident, $verified:ident) => {{
        use crate::locators::*;

        match $env {
            Environment::Test(_) => {
                $l1::contribution_locator_exists($env, $round_height, $chunk_id, $cont_id, $verified)
            }
            Environment::Development(_) => {
                $l2::contribution_locator_exists($env, $round_height, $chunk_id, $cont_id, $verified)
            }
            Environment::Production(_) => {
                $l3::contribution_locator_exists($env, $round_height, $chunk_id, $cont_id, $verified)
            }
        }
    }};
}

/// Returns the round locator using a locator that is determined based
/// on the environment the coordinator is operating in.
#[macro_export]
macro_rules! round_locator {
    ($env:ident, $l1:ident, $l2:ident, $l3:ident, $round_height:ident) => {{
        use crate::locators::*;

        match $env {
            Environment::Test(_) => <$l1 as Locator>::round_locator($env, $round_height),
            Environment::Development(_) => <$l2 as Locator>::round_locator($env, $round_height),
            Environment::Production(_) => <$l3 as Locator>::round_locator($env, $round_height),
        }
    }};
}

/// Returns `true` if the round locator exists using a locator that is determined based
/// on the environment the coordinator is operating in.
#[macro_export]
macro_rules! round_locator_exists {
    ($env:ident, $l1:ident, $l2:ident, $l3:ident, $round_height:ident) => {{
        use crate::locators::*;

        match $env {
            Environment::Test(_) => $l1::round_locator_exists($env, $round_height),
            Environment::Development(_) => $l2::round_locator_exists($env, $round_height),
            Environment::Production(_) => $l3::round_locator_exists($env, $round_height),
        }
    }};
}

/// Returns an error logging message using `tracing`, then returns the error itself.
#[macro_export]
macro_rules! return_error {
    ($error:ident, $message:ident) => {{
        error!($message);
        return $error;
    }};
}
