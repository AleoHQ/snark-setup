use phase1::{Phase1, Phase1Parameters};
use setup_utils::{calculate_hash, print_hash, CheckForCorrectness, UseCompression};

use snarkvm_curves::PairingEngine as Engine;

use memmap::*;
use rand::Rng;
use std::{
    fs::OpenOptions,
    io::{Read, Write},
};

pub fn contribute<T: Engine + Sync>(
    compressed_input: UseCompression,
    challenge_filename: &str,
    compressed_output: UseCompression,
    response_filename: &str,
    check_input_correctness: CheckForCorrectness,
    parameters: &Phase1Parameters<T>,
    mut rng: impl Rng,
) {
    // Try to load challenge file from disk.
    let reader = OpenOptions::new()
        .read(true)
        .open(challenge_filename)
        .expect("unable open challenge file");
    {
        let metadata = reader
            .metadata()
            .expect("unable to get filesystem metadata for challenge file");
        let expected_challenge_length = match compressed_input {
            UseCompression::Yes => parameters.contribution_size - parameters.public_key_size,
            UseCompression::No => parameters.accumulator_size,
        };

        if metadata.len() != (expected_challenge_length as u64) {
            panic!(
                "The size of challenge file should be {}, but it's {}, so something isn't right.",
                expected_challenge_length,
                metadata.len()
            );
        }
    }

    let readable_map = unsafe {
        MmapOptions::new()
            .map(&reader)
            .expect("unable to create a memory map for input")
    };

    // Create response file in this directory
    let writer = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(response_filename)
        .expect("unable to create response file");

    let required_output_length = match compressed_output {
        UseCompression::Yes => parameters.contribution_size,
        UseCompression::No => parameters.accumulator_size + parameters.public_key_size,
    };

    writer
        .set_len(required_output_length as u64)
        .expect("must make output file large enough");

    let mut writable_map = unsafe {
        MmapOptions::new()
            .map_mut(&writer)
            .expect("unable to create a memory map for output")
    };

    println!("Calculating previous contribution hash...");

    assert!(
        UseCompression::No == compressed_input,
        "Hashing the compressed file in not yet defined"
    );
    let current_accumulator_hash = calculate_hash(&readable_map);

    {
        println!("`challenge` file contains decompressed points and has a hash:");
        print_hash(&current_accumulator_hash);

        (&mut writable_map[0..])
            .write_all(current_accumulator_hash.as_slice())
            .expect("unable to write a challenge hash to mmap");

        writable_map.flush().expect("unable to write hash to response file");
    }

    {
        let mut challenge_hash = [0; 64];
        let mut memory_slice = readable_map.get(0..64).expect("must read point data from file");
        memory_slice
            .read_exact(&mut challenge_hash)
            .expect("couldn't read hash of challenge file from response file");

        println!(
            "`challenge` file claims (!!! Must not be blindly trusted) that it was based on the original contribution with a hash:"
        );
        print_hash(&challenge_hash);
    }

    // Construct our keypair using the RNG we created above
    let (public_key, private_key) =
        Phase1::key_generation(&mut rng, current_accumulator_hash.as_ref()).expect("could not generate keypair");

    // Perform the transformation
    println!("Computing and writing your contribution, this could take a while...");

    // this computes a transformation and writes it
    Phase1::computation(
        &readable_map,
        &mut writable_map,
        compressed_input,
        compressed_output,
        check_input_correctness,
        &private_key,
        &parameters,
    )
    .expect("must contribute with the key");

    println!("Finishing writing your contribution to response file...");

    // Write the public key
    public_key
        .write(&mut writable_map, compressed_output, &parameters)
        .expect("unable to write public key");

    writable_map.flush().expect("must flush a memory map");

    // Get the hash of the contribution, so the user can compare later
    let output_readonly = writable_map.make_read_only().expect("must make a map readonly");
    let contribution_hash = calculate_hash(&output_readonly);

    print!(
        "Done!\n\n\
              Your contribution has been written to response file\n\n\
              The BLAKE2b hash of response file is:\n"
    );
    print_hash(&contribution_hash);
    println!("Thank you for your participation, much appreciated! :)");
}
