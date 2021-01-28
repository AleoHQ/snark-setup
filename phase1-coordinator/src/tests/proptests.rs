use crate::{
    authentication::Dummy,
    commands::Seed,
    coordinator_state::ParticipantInfo,
    environment::{Parameters, Testing},
    testing::initialize_test_environment_with_debug,
    Coordinator,
    Participant,
};
use phase1::{helpers::CurveKind, ContributionMode, ProvingSystem};

use anyhow::Context;
use proptest::{
    prelude::{any, ProptestConfig},
    strategy::Strategy,
};
use proptest_derive::Arbitrary;
use tracing::info;

use std::{collections::HashMap, panic};

use super::{create_contributor, create_verifier};

/// Info about a [Particpant] (verifier or contributor) to be created
/// and used in a proptest.
#[derive(Debug, Arbitrary, Clone)]
struct ParticipantProptestParams {
    reliability_score: u8,
}

/// Info about a contributor being used in a proptest.
struct ContributorProptestInfo {
    /// Parameters used to create this contributor.
    params: ParticipantProptestParams,
    signing_key: String,
    seed: Seed,
}

/// Info about a verifier being used in a proptest.
struct VerifierProptestInfo {
    /// Parameters used to create this verifier.
    params: ParticipantProptestParams,
    signing_key: String,
}

proptest::prop_compose! {
    /// Generate [Parameters::Custom] to use in a [proptest].
    fn env_parameters_custom_strategy()(
        // batch sizes <= 1 are not supported
        batch_size in 2..16usize,
        chunk_size in 1..16usize
    ) -> Parameters {
        Parameters::Custom((
            ContributionMode::Chunked,
            ProvingSystem::Groth16,
            CurveKind::Bls12_377,
            6,  /* power */
            batch_size,
            chunk_size,
        ))
    }
}

#[derive(Clone, Debug)]
struct RoundProptestParams {
    rounds: usize,
    contributors: Vec<Vec<ParticipantProptestParams>>,
    verifiers: Vec<Vec<ParticipantProptestParams>>,
}

fn rounds_strategy(
    max_rounds: usize,
    max_contributors_per_round: usize,
    max_verifiers_per_round: usize,
) -> impl Strategy<Value = RoundProptestParams> {
    let rounds_strategy = 1..max_rounds;
    rounds_strategy
        .prop_ind_flat_map2(move |rounds| {
            let contributors_strategy = proptest::collection::vec(
                proptest::collection::vec(any::<ParticipantProptestParams>(), 1..max_contributors_per_round),
                rounds,
            );

            let verifiers_strategy = proptest::collection::vec(
                proptest::collection::vec(any::<ParticipantProptestParams>(), 1..max_verifiers_per_round),
                rounds,
            );

            (contributors_strategy, verifiers_strategy)
        })
        .prop_filter(
            "contributors length must match number of rounds",
            |(rounds, (contributors, _verifiers))| contributors.len() == *rounds,
        )
        .prop_filter(
            "verifiers length must match number of rounds",
            |(rounds, (_contributors, verifiers))| verifiers.len() == *rounds,
        )
        .prop_map(|(rounds, (contributors, verifiers))| RoundProptestParams {
            rounds,
            contributors,
            verifiers,
        })
}

proptest::proptest! {
    #![proptest_config(ProptestConfig::with_cases(5))]

    /// This test has a low number of cases because it takes so long
    /// to run, so it may fail intermittently if there is a bug.
    /// **Please do not ignore spurious failures in CI or your
    /// personal testing**. If there is a failure [it should produce a
    /// persistence file](https://altsysrq.github.io/proptest-book/proptest/failure-persistence.html)
    /// that you should consider checking into source control.
    #[test]
    fn coordinator_proptest(
        env_params in env_parameters_custom_strategy(),
        rounds_params in rounds_strategy(
            3, // max rounds
            5, // max contributors per round
            5  // max verifiers per round
        ),
    ) {
        coordinator_proptest_impl(
            env_params,
            rounds_params)
            .context("Error during proptest")
            .unwrap();
    }
}

fn proptest_add_contributor(
    coordinator: &Coordinator,
    contributor: Participant,
    proptest_info: &ContributorProptestInfo,
) -> anyhow::Result<()> {
    coordinator
        .add_to_queue(contributor.clone(), proptest_info.params.reliability_score)
        .map_err(anyhow::Error::from)
        .context("error while adding contributor to coordinator")?;
    assert!(coordinator.is_queue_contributor(&contributor));
    assert!(!coordinator.is_current_contributor(&contributor));
    assert!(!coordinator.is_finished_contributor(&contributor));
    Ok(())
}

fn proptest_add_verifier(
    coordinator: &Coordinator,
    verifier: Participant,
    proptest_info: &VerifierProptestInfo,
) -> anyhow::Result<()> {
    coordinator
        .add_to_queue(verifier.clone(), proptest_info.params.reliability_score)
        .map_err(anyhow::Error::from)
        .context("error while adding verifier to coordinator")?;
    assert!(coordinator.is_queue_verifier(&verifier));
    assert!(!coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_verifier(&verifier));
    Ok(())
}

fn get_current_participant_info(
    coordinator: &Coordinator,
    participant: &Participant,
) -> anyhow::Result<ParticipantInfo> {
    let result = match participant {
        Participant::Contributor(_) => coordinator.current_contributor_info(participant),
        Participant::Verifier(_) => coordinator.current_verifier_info(participant),
    };

    result
        .map_err(anyhow::Error::from)
        .with_context(|| format!("error obtaining current info for participant {}", participant))?
        .ok_or_else(|| anyhow::anyhow!("no current info for participant {}", participant))
}

// Implementation is in a seperate function so VSCode gives proper
// syntax highlighting/metadata.
fn coordinator_proptest_impl(env_params: Parameters, rounds_params: RoundProptestParams) -> anyhow::Result<()> {
    dbg!(&env_params);
    dbg!(&rounds_params);

    // Create a new test environment.
    let environment = initialize_test_environment_with_debug(&Testing::from(env_params).into());

    // Instantiate a coordinator.
    let coordinator = Coordinator::new(environment, Box::new(Dummy))
        .map_err(anyhow::Error::from)
        .context("error while creating coordinator")?;

    // Initialize the ceremony to round 0.
    coordinator
        .initialize()
        .map_err(anyhow::Error::from)
        .context("error initializing coordinator")?;

    assert_eq!(
        0,
        coordinator
            .current_round_height()
            .map_err(anyhow::Error::from)
            .context("error obtaining current round height")?
    );

    for round in 1..=rounds_params.rounds {
        let span = tracing::error_span!("proptest_round", round = round);
        let _guard = span.enter();

        info!("Adding contributors new round");

        let contributors: HashMap<Participant, ContributorProptestInfo> = rounds_params
            .contributors
            .get(round - 1)
            .expect("contributors should be available for every round")
            .iter()
            .enumerate()
            .map(|(i, params)| {
                let id = format!("r{}i{}", round, i);
                let (contributor, signing_key, seed) = create_contributor(&id);

                let info = ContributorProptestInfo {
                    params: params.clone(),
                    signing_key,
                    seed,
                };

                (contributor, info)
            })
            .collect();

        for (contributor, proptest_info) in &contributors {
            proptest_add_contributor(&coordinator, contributor.clone(), proptest_info)?;
        }

        assert_eq!(contributors.len(), coordinator.number_of_queue_contributors());
        assert!(contributors.len() > 0);

        info!("Adding verifiers new round");

        let verifiers: HashMap<Participant, VerifierProptestInfo> = rounds_params
            .verifiers
            .get(round - 1)
            .expect("verifiers should be available for every round")
            .iter()
            .enumerate()
            .map(|(i, params)| {
                let id = format!("r{}i{}", round, i);
                let (verifier, signing_key) = create_verifier(&id);

                let info = VerifierProptestInfo {
                    params: params.clone(),
                    signing_key,
                };

                (verifier, info)
            })
            .collect();

        for (verifier, proptest_info) in &verifiers {
            proptest_add_verifier(&coordinator, verifier.clone(), proptest_info)?;
        }

        assert_eq!(verifiers.len(), coordinator.number_of_queue_verifiers());
        assert!(verifiers.len() > 0);

        assert_eq!(
            (round as u64) - 1,
            coordinator
                .current_round_height()
                .map_err(anyhow::Error::from)
                .context("error obtaining current round height")?
        );

        info!("Updating ceremony");
        coordinator
            .update()
            .map_err(anyhow::Error::from)
            .with_context(|| format!("error while updating coordinator during round {}", round))?;

        assert_eq!(
            round as u64,
            coordinator
                .current_round_height()
                .map_err(anyhow::Error::from)
                .context("error obtaining current round height")?
        );

        for (contributor, proptest_info) in &contributors {
            tracing::error_span!("{} contribution", %contributor);
            tracing::debug!("making contribution");

            // Calculate how many contributions this contributor needs
            // to make this round.
            let (start_n_assigned_tasks, start_n_completed_tasks) = {
                let contributor_info = get_current_participant_info(&coordinator, &contributor)?;
                (
                    contributor_info.assigned_tasks().len(),
                    contributor_info.completed_tasks().len(),
                )
            };

            // Perform the contributions.
            for _ in 0..start_n_assigned_tasks {
                coordinator
                    .contribute(&contributor, &proptest_info.signing_key, &proptest_info.seed)
                    .map_err(anyhow::Error::from)
                    .with_context(|| format!("Error while contributor {} was making contribution", contributor))?;
            }

            // Check that the contributor has finished all their contributions for this round.
            {
                let contributor_info = get_current_participant_info(&coordinator, &contributor)?;

                assert!(contributor_info.assigned_tasks().is_empty());
                assert_eq!(
                    start_n_completed_tasks + start_n_assigned_tasks,
                    contributor_info.completed_tasks().len()
                );
            }

            assert!(coordinator.is_current_contributor(contributor));
            assert!(coordinator.is_finished_contributor(contributor));
        }

        for (verifier, proptest_info) in &verifiers {
            tracing::error_span!("{} verification", %verifier);
            tracing::debug!("performing verification");

            // Calculate how many verifications need to occur this round.
            let (start_n_assigned_tasks, start_n_completed_tasks) = {
                let verifier_info = get_current_participant_info(&coordinator, &verifier)?;
                (
                    verifier_info.assigned_tasks().len(),
                    verifier_info.completed_tasks().len(),
                )
            };

            // Perform the verifications for this round.
            for _ in 0..start_n_assigned_tasks {
                coordinator
                    .verify(&verifier, &proptest_info.signing_key)
                    .map_err(anyhow::Error::from)
                    .with_context(|| format!("Error while verifier {} was performing verification", verifier))?;
            }

            // Check that the verifier has finished all their verification tasks for this round.
            {
                let verifier_info = get_current_participant_info(&coordinator, &verifier)?;
                assert!(verifier_info.assigned_tasks().is_empty());
                assert_eq!(
                    start_n_completed_tasks + start_n_assigned_tasks,
                    verifier_info.completed_tasks().len()
                );
            }

            assert!(coordinator.is_current_verifier(verifier));
            assert!(coordinator.is_finished_verifier(verifier));
        }
    }

    Ok(())
}
