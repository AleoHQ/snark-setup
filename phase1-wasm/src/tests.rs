use crate::phase1::*;
use phase1::{ContributionMode, Phase1, Phase1Parameters, ProvingSystem};
use setup_utils::{batch_exp, blank_hash, generate_powers_of_tau, UseCompression};
use snarkvm_curves::{bls12_377::Bls12_377, bw6_761::BW6_761, AffineCurve, PairingEngine, ProjectiveCurve};
use snarkvm_fields::{batch_inversion, Field};

use rand::SeedableRng;
use rand_xorshift::XorShiftRng;
use wasm_bindgen_test::*;

fn generate_input<E: PairingEngine>(
    parameters: &Phase1Parameters<E>,
    compressed: UseCompression,
) -> (Vec<u8>, Phase1<E>) {
    let length = parameters.get_length(compressed);

    let mut output = vec![0; length];
    Phase1::initialization(&mut output, compressed, &parameters).unwrap();

    let mut input = vec![0; length];
    input.copy_from_slice(&output);

    let before = Phase1::deserialize(&output, compressed, CHECK_INPUT_CORRECTNESS, &parameters).unwrap();
    (input, before)
}

fn contribute_challenge_test<E: PairingEngine + Sync>(parameters: &Phase1Parameters<E>) {
    // Get a non-mutable copy of the initial accumulator state.
    let (input, mut before) = generate_input(&parameters, COMPRESSED_INPUT);

    let mut rng = XorShiftRng::seed_from_u64(0);
    // Construct our keypair using the RNG we created above
    let current_accumulator_hash = blank_hash();

    let (_, privkey): (phase1::PublicKey<E>, phase1::PrivateKey<E>) =
        Phase1::key_generation(&mut rng, current_accumulator_hash.as_ref()).expect("could not generate keypair");

    let output = contribute_challenge(&input, parameters, XorShiftRng::seed_from_u64(0))
        .unwrap()
        .response;

    let deserialized = Phase1::deserialize(&output, COMPRESSED_OUTPUT, CHECK_INPUT_CORRECTNESS, &parameters).unwrap();

    let (min, max) = match parameters.contribution_mode {
        ContributionMode::Full => match parameters.proving_system {
            ProvingSystem::Groth16 => (0, parameters.powers_g1_length),
            ProvingSystem::Marlin => (0, parameters.powers_length),
        },
        ContributionMode::Chunked => match parameters.proving_system {
            ProvingSystem::Groth16 => (
                parameters.chunk_index * parameters.chunk_size,
                std::cmp::min(
                    parameters.powers_g1_length,
                    (parameters.chunk_index + 1) * parameters.chunk_size,
                ),
            ),
            ProvingSystem::Marlin => (
                parameters.chunk_index * parameters.chunk_size,
                std::cmp::min(
                    parameters.powers_length,
                    (parameters.chunk_index + 1) * parameters.chunk_size,
                ),
            ),
        },
    };

    match parameters.proving_system {
        ProvingSystem::Groth16 => {
            let tau_powers = generate_powers_of_tau::<E>(&privkey.tau, min, max);
            batch_exp(
                &mut before.tau_powers_g1,
                &tau_powers[0..parameters.g1_chunk_size],
                None,
            )
            .unwrap();
            batch_exp(
                &mut before.tau_powers_g2,
                &tau_powers[0..parameters.other_chunk_size],
                None,
            )
            .unwrap();
            batch_exp(
                &mut before.alpha_tau_powers_g1,
                &tau_powers[0..parameters.other_chunk_size],
                Some(&privkey.alpha),
            )
            .unwrap();
            batch_exp(
                &mut before.beta_tau_powers_g1,
                &tau_powers[0..parameters.other_chunk_size],
                Some(&privkey.beta),
            )
            .unwrap();
            before.beta_g2 = before.beta_g2.mul(privkey.beta).into_affine();
        }
        ProvingSystem::Marlin => {
            let tau_powers = generate_powers_of_tau::<E>(&privkey.tau, min, max);
            batch_exp(
                &mut before.tau_powers_g1,
                &tau_powers[0..parameters.g1_chunk_size],
                None,
            )
            .unwrap();

            if parameters.chunk_index == 0 || parameters.contribution_mode == ContributionMode::Full {
                let degree_bound_powers = (0..parameters.total_size_in_log2)
                    .map(|i| privkey.tau.pow([parameters.powers_length as u64 - 1 - (1 << i) + 2]))
                    .collect::<Vec<_>>();
                let mut g2_inverse_powers = degree_bound_powers.clone();
                batch_inversion(&mut g2_inverse_powers);
                batch_exp(&mut before.tau_powers_g2[..2], &tau_powers[0..2], None).unwrap();
                batch_exp(
                    &mut before.tau_powers_g2[2..],
                    &g2_inverse_powers[0..parameters.total_size_in_log2],
                    None,
                )
                .unwrap();

                let g1_degree_powers = degree_bound_powers
                    .into_iter()
                    .map(|f| vec![f, f * &privkey.tau, f * &privkey.tau * &privkey.tau])
                    .flatten()
                    .collect::<Vec<_>>();
                batch_exp(
                    &mut before.alpha_tau_powers_g1[3..3 + 3 * parameters.total_size_in_log2],
                    &g1_degree_powers,
                    Some(&privkey.alpha),
                )
                .unwrap();
                batch_exp(
                    &mut before.alpha_tau_powers_g1[0..3],
                    &tau_powers[0..3],
                    Some(&privkey.alpha),
                )
                .unwrap();
            }
        }
    }
    assert_eq!(deserialized, before);
}

#[wasm_bindgen_test]
pub fn test_phase1_contribute_bls12_377_full() {
    for proving_system in &[ProvingSystem::Groth16, ProvingSystem::Marlin] {
        contribute_challenge_test(&get_parameters_full::<Bls12_377>(*proving_system, 2, 2));
        // Works even when the batch is larger than the powers
        contribute_challenge_test(&get_parameters_full::<Bls12_377>(*proving_system, 6, 128));
    }
}

#[wasm_bindgen_test]
fn test_phase1_contribute_bw6_761_full() {
    for proving_system in &[ProvingSystem::Groth16, ProvingSystem::Marlin] {
        contribute_challenge_test(&get_parameters_full::<BW6_761>(*proving_system, 2, 2));
        // Works even when the batch is larger than the powers
        contribute_challenge_test(&get_parameters_full::<BW6_761>(*proving_system, 6, 128));
    }
}

#[wasm_bindgen_test]
pub fn test_phase1_contribute_bls12_377_chunked() {
    for proving_system in &[ProvingSystem::Groth16, ProvingSystem::Marlin] {
        let powers = 10;
        let chunk_size = 3 + 3 * powers + 1; // to ensure the Marlin extra elements fit in chunk 0
        let num_chunks = match *proving_system {
            ProvingSystem::Groth16 => (((1 << powers) << 1) - 1 + chunk_size - 1) / chunk_size,
            ProvingSystem::Marlin => ((1 << powers) + chunk_size - 1) / chunk_size,
        };
        for i in 0..num_chunks {
            contribute_challenge_test(&get_parameters_chunked::<Bls12_377>(
                *proving_system,
                powers,
                2,
                i,
                chunk_size,
            ));
        }
    }
}

#[wasm_bindgen_test]
fn test_phase1_contribute_bw6_761_chunked() {
    for proving_system in &[ProvingSystem::Groth16, ProvingSystem::Marlin] {
        let powers = 10;
        let chunk_size = 3 + 3 * powers + 1; // to ensure the Marlin extra elements fit in chunk 0
        let num_chunks = match *proving_system {
            ProvingSystem::Groth16 => (((1 << powers) << 1) - 1 + chunk_size - 1) / chunk_size,
            ProvingSystem::Marlin => ((1 << powers) + chunk_size - 1) / chunk_size,
        };
        for i in 0..num_chunks {
            contribute_challenge_test(&get_parameters_chunked::<BW6_761>(
                *proving_system,
                powers,
                2,
                i,
                chunk_size,
            ));
        }
    }
}
