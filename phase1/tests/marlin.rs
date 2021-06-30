#[cfg(test)]
mod test {
    use phase1::{helpers::testing::generate_input, Phase1, Phase1Parameters, ProvingSystem};
    use rand::thread_rng;
    use setup_utils::{blank_hash, CheckForCorrectness, UseCompression};

    use snarkvm_algorithms::SNARK;
    use snarkvm_curves::{
        bls12_377::{Bls12_377, Fr},
        PairingCurve,
    };
    use snarkvm_fields::Field;
    use snarkvm_polycommit::kzg10::UniversalParams;
    use snarkvm_posw::{txids_to_roots, Marlin, PoswMarlin};
    use snarkvm_r1cs::{ConstraintSynthesizer, ConstraintSystem, SynthesisError};
    use snarkvm_utilities::{serialize::*, UniformRand};

    use itertools::Itertools;
    use std::{collections::BTreeMap, ops::MulAssign};

    #[test]
    fn test_marlin_posw_bls12_377() {
        let powers = 19usize;
        let batch = 1usize << 16;
        let parameters = Phase1Parameters::<Bls12_377>::new_full(ProvingSystem::Marlin, powers, batch);
        let expected_response_length = parameters.get_length(UseCompression::No);

        // Get a non-mutable copy of the initial accumulator state.
        let (input, _) = generate_input(&parameters, UseCompression::No, CheckForCorrectness::No);

        let mut output = vec![0; expected_response_length];

        // Construct our keypair using the RNG we created above
        let current_accumulator_hash = blank_hash();
        let mut rng = thread_rng();
        let (_, privkey) =
            Phase1::key_generation(&mut rng, current_accumulator_hash.as_ref()).expect("could not generate keypair");

        Phase1::computation(
            &input,
            &mut output,
            UseCompression::No,
            UseCompression::No,
            CheckForCorrectness::No,
            &privkey,
            &parameters,
        )
        .unwrap();

        let deserialized =
            Phase1::deserialize(&output, UseCompression::No, CheckForCorrectness::No, &parameters).unwrap();
        let tau_powers_g1 = deserialized.tau_powers_g1;
        let tau_powers_g2 = deserialized.tau_powers_g2;
        let alpha_powers_g1 = deserialized.alpha_tau_powers_g1;

        let mut alpha_tau_powers_g1 = BTreeMap::new();
        for i in 0..3 {
            alpha_tau_powers_g1.insert(i, alpha_powers_g1[i]);
        }
        alpha_powers_g1[3..]
            .iter()
            .chunks(3)
            .into_iter()
            .enumerate()
            .for_each(|(i, c)| {
                let c = c.into_iter().collect::<Vec<_>>();
                alpha_tau_powers_g1.insert(parameters.powers_length - 1 - (1 << i) + 2, *c[0]);
                alpha_tau_powers_g1.insert(parameters.powers_length - 1 - (1 << i) + 3, *c[1]);
                alpha_tau_powers_g1.insert(parameters.powers_length - 1 - (1 << i) + 4, *c[2]);
            });

        let mut prepared_neg_powers_of_h = BTreeMap::new();
        tau_powers_g2[2..].iter().enumerate().for_each(|(i, p)| {
            prepared_neg_powers_of_h.insert(parameters.powers_length - 1 - (1 << i) + 2, p.prepare());
        });

        let h = tau_powers_g2[0].clone();
        let beta_h = tau_powers_g2[1].clone();

        let universal_params = UniversalParams::<Bls12_377> {
            powers_of_g: tau_powers_g1,
            powers_of_gamma_g: alpha_tau_powers_g1,
            h: h.clone(),
            beta_h: beta_h.clone(),
            prepared_neg_powers_of_h,
            prepared_h: h.prepare(),
            prepared_beta_h: beta_h.prepare(),
        };

        let posw = PoswMarlin::index(universal_params).unwrap();

        // super low difficulty so we find a solution immediately
        let difficulty_target = 0xFFFF_FFFF_FFFF_FFFF_u64;

        let transaction_ids = vec![[1u8; 32]; 8];
        let (_, pedersen_merkle_root, subroots) = txids_to_roots(&transaction_ids);

        // generate the proof
        let (nonce, proof) = posw
            .mine(&subroots, difficulty_target, &mut rand::thread_rng(), std::u32::MAX)
            .unwrap();

        assert_eq!(proof.len(), 972); // NOTE: Marlin proofs use compressed serialization

        let proof = <Marlin<Bls12_377> as SNARK>::Proof::read(&proof[..]).unwrap();
        posw.verify(nonce, &proof, &pedersen_merkle_root).unwrap();
    }

    #[derive(Copy, Clone)]
    struct Circuit<F: Field> {
        a: Option<F>,
        b: Option<F>,
        num_constraints: usize,
        num_variables: usize,
    }

    impl<ConstraintF: Field> ConstraintSynthesizer<ConstraintF> for Circuit<ConstraintF> {
        fn generate_constraints<CS: ConstraintSystem<ConstraintF>>(&self, cs: &mut CS) -> Result<(), SynthesisError> {
            let a = cs.alloc(|| "a", || self.a.ok_or(SynthesisError::AssignmentMissing))?;
            let b = cs.alloc(|| "b", || self.b.ok_or(SynthesisError::AssignmentMissing))?;
            let c = cs.alloc_input(
                || "c",
                || {
                    let mut a = self.a.ok_or(SynthesisError::AssignmentMissing)?;
                    let b = self.b.ok_or(SynthesisError::AssignmentMissing)?;

                    a.mul_assign(&b);
                    Ok(a)
                },
            )?;

            for i in 0..(self.num_variables - 3) {
                let _ = cs.alloc(
                    || format!("var {}", i),
                    || self.a.ok_or(SynthesisError::AssignmentMissing),
                )?;
            }

            for i in 0..self.num_constraints {
                cs.enforce(|| format!("constraint {}", i), |lc| lc + a, |lc| lc + b, |lc| lc + c);
            }
            Ok(())
        }
    }

    type MarlinInst = snarkvm_marlin::MarlinTestnet1<Bls12_377>;

    #[test]
    fn test_marlin_sonic_pc() {
        let powers = 16usize;
        let batch = 1usize << 12;
        let parameters = Phase1Parameters::<Bls12_377>::new_full(ProvingSystem::Marlin, powers, batch);
        let expected_response_length = parameters.get_length(UseCompression::No);

        // Get a non-mutable copy of the initial accumulator state.
        let (input, _) = generate_input(&parameters, UseCompression::No, CheckForCorrectness::No);

        let mut output = vec![0; expected_response_length];

        // Construct our keypair using the RNG we created above
        let current_accumulator_hash = blank_hash();
        let mut rng = thread_rng();
        let (_, privkey) =
            Phase1::key_generation(&mut rng, current_accumulator_hash.as_ref()).expect("could not generate keypair");

        Phase1::computation(
            &input,
            &mut output,
            UseCompression::No,
            UseCompression::No,
            CheckForCorrectness::No,
            &privkey,
            &parameters,
        )
        .unwrap();

        let deserialized =
            Phase1::deserialize(&output, UseCompression::No, CheckForCorrectness::No, &parameters).unwrap();
        let tau_powers_g1 = deserialized.tau_powers_g1;
        let tau_powers_g2 = deserialized.tau_powers_g2;
        let alpha_powers_g1 = deserialized.alpha_tau_powers_g1;

        let mut alpha_tau_powers_g1 = BTreeMap::new();
        for i in 0..3 {
            alpha_tau_powers_g1.insert(i, alpha_powers_g1[i]);
        }
        alpha_powers_g1[3..]
            .iter()
            .chunks(3)
            .into_iter()
            .enumerate()
            .for_each(|(i, c)| {
                let c = c.into_iter().collect::<Vec<_>>();
                alpha_tau_powers_g1.insert(parameters.powers_length - 1 - (1 << i) + 2, *c[0]);
                alpha_tau_powers_g1.insert(parameters.powers_length - 1 - (1 << i) + 3, *c[1]);
                alpha_tau_powers_g1.insert(parameters.powers_length - 1 - (1 << i) + 4, *c[2]);
            });

        let mut prepared_neg_powers_of_h = BTreeMap::new();
        tau_powers_g2[2..].iter().enumerate().for_each(|(i, p)| {
            prepared_neg_powers_of_h.insert(parameters.powers_length - 1 - (1 << i) + 2, p.prepare());
        });
        let h = tau_powers_g2[0].clone();
        let beta_h = tau_powers_g2[1].clone();
        let universal_params = UniversalParams::<Bls12_377> {
            powers_of_g: tau_powers_g1,
            powers_of_gamma_g: alpha_tau_powers_g1,
            h: h.clone(),
            beta_h: beta_h.clone(),
            prepared_neg_powers_of_h,
            prepared_h: h.prepare(),
            prepared_beta_h: beta_h.prepare(),
        };

        for _ in 0..100 {
            let a = Fr::rand(&mut rng);
            let b = Fr::rand(&mut rng);
            let mut c = a;
            c.mul_assign(&b);

            let circuit = Circuit {
                a: Some(a),
                b: Some(b),
                num_constraints: 3000,
                num_variables: 2000,
            };

            let (index_pk, index_vk) = MarlinInst::circuit_setup(&universal_params, &circuit).unwrap();
            println!("Called circuit setup");

            let proof = MarlinInst::prove(&index_pk, &circuit, &mut rng).unwrap();
            println!("Called prover");

            assert!(MarlinInst::verify(&index_vk, &[c], &proof).unwrap());
            println!("Called verifier");
            println!("\nShould not verify (i.e. verifier messages should print below):");
            assert!(!MarlinInst::verify(&index_vk, &[a], &proof).unwrap());
        }
    }
}
