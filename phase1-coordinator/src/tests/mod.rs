use crate::{
    authentication::Dummy,
    commands::{Seed, SigningKey, SEED_LENGTH},
    coordinator_state::{ParticipantInfo, Task},
    environment::{Parameters, Testing},
    storage::{Locator, Object},
    testing::prelude::*,
    Coordinator,
    Participant,
};
use phase1::{helpers::CurveKind, ContributionMode, ProvingSystem};

use rand::RngCore;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use std::{collections::HashSet, panic};

pub mod proptests;

#[inline]
fn create_contributor(id: &str) -> (Participant, SigningKey, Seed) {
    let contributor = Participant::Contributor(format!("test-contributor-{}", id));
    let contributor_signing_key: SigningKey = "secret_key".to_string();

    let mut seed: Seed = [0; SEED_LENGTH];
    rand::thread_rng().fill_bytes(&mut seed[..]);

    (contributor, contributor_signing_key, seed)
}

#[inline]
fn create_verifier(id: &str) -> (Participant, SigningKey) {
    let verifier = Participant::Verifier(format!("test-verifier-{}", id));
    let verifier_signing_key: SigningKey = "secret_key".to_string();

    (verifier, verifier_signing_key)
}

fn execute_round_test(proving_system: ProvingSystem, curve: CurveKind) -> anyhow::Result<()> {
    let parameters = Parameters::Custom((
        ContributionMode::Chunked,
        proving_system,
        curve,
        7,  /* power */
        32, /* batch_size */
        32, /* chunk_size */
    ));
    let environment = initialize_test_environment_with_debug(&Testing::from(parameters).into());
    let number_of_chunks = environment.number_of_chunks() as usize;

    // Instantiate a coordinator.
    let coordinator = Coordinator::new(environment, Box::new(Dummy))?;

    // Initialize the ceremony to round 0.
    coordinator.initialize()?;
    assert_eq!(0, coordinator.current_round_height()?);

    // Meanwhile, add a contributor and verifier to the queue.
    let (contributor, contributor_signing_key, seed) = create_contributor("1");
    let (verifier, verifier_signing_key) = create_verifier("1");
    coordinator.add_to_queue(contributor.clone(), 10)?;
    coordinator.add_to_queue(verifier.clone(), 10)?;
    assert_eq!(1, coordinator.number_of_queue_contributors());
    assert_eq!(1, coordinator.number_of_queue_verifiers());

    // Advance the ceremony from round 0 to round 1.
    coordinator.update()?;
    assert_eq!(1, coordinator.current_round_height()?);
    assert_eq!(0, coordinator.number_of_queue_contributors());
    assert_eq!(0, coordinator.number_of_queue_verifiers());

    // Run contribution and verification for round 1.
    for _ in 0..number_of_chunks {
        coordinator.contribute(&contributor, &contributor_signing_key, &seed)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
    }

    //
    // Meanwhile, add a contributor and verifier to the queue.
    //
    // Note: This logic for adding to the queue works because
    // `Environment::allow_current_contributors_in_queue`
    // and `Environment::allow_current_verifiers_in_queue`
    // are set to `true`. This section can be removed without
    // changing the outcome of this test, if necessary.
    //
    let (contributor, _, _) = create_contributor("1");
    let (verifier, _) = create_verifier("1");
    coordinator.add_to_queue(contributor.clone(), 10)?;
    coordinator.add_to_queue(verifier.clone(), 10)?;
    assert_eq!(1, coordinator.number_of_queue_contributors());
    assert_eq!(1, coordinator.number_of_queue_verifiers());

    // Update the ceremony from round 1 to round 2.
    coordinator.update()?;
    assert_eq!(2, coordinator.current_round_height()?);
    assert_eq!(0, coordinator.number_of_queue_contributors());
    assert_eq!(0, coordinator.number_of_queue_verifiers());

    Ok(())
}

/*
    Drop Participant Tests

    1. Basic drop - `test_coordinator_drop_contributor_basic`
        Drop a contributor that does not affect other contributors/verifiers.

    2. Given 3 contributors, drop middle contributor - `test_coordinator_drop_contributor_in_between_two_contributors`
        Given contributors 1, 2, and 3, drop contributor 2 and ensure that the tasks are present.

    3. Drop contributor with pending tasks - `test_coordinator_drop_contributor_with_contributors_in_pending_tasks`
        Drops a contributor with other contributors in pending tasks.

    4. Drop contributor with a locked chunk - `test_coordinator_drop_contributor_with_locked_chunk`
        Test that dropping a contributor releases the locks held by the dropped contributor.

    5. Dropping a contributor removes provided contributions - `test_coordinator_drop_contributor_removes_contributions`
        Test that dropping a contributor will remove all the contributions that the dropped contributor has provided.

    6. Dropping a participant clears lock for subsequent contributors/verifiers - `test_coordinator_drop_contributor_clear_locks`
        Test that if a contribution is dropped from a chunk, while a  contributor/verifier is performing their contribution,
        the lock should be released after the task has been disposed. The disposed task should also be reassigned correctly.
        Currently, the lock is release and the task is disposed after the contributor/verifier calls `try_contribute` or `try_verify`.

    7. Dropping a contributor removes all subsequent contributions  - UNTESTED
        If a contributor is dropped, all contributions built on top of the dropped contributions must also
        be dropped.

    8. Dropping multiple contributors allocates tasks to the coordinator contributor correctly - FAILING `test_coordinator_drop_multiple_contributors`
        Pick contributor with least load in `add_replacement_contributor_unsafe`.
        May need some interleaving logic

    9. Current contributor/verifier `completed_tasks` should be removed/moved when a participant is dropped
       and tasks need to be recomputed - UNTESTED
        The tasks declared in the state file should be updated correctly when a participant is dropped.

    10. The coordinator contributor should replace all dropped participants and complete the round correctly. - UNTESTED

    11. Drop one contributor and check that completed tasks are reassigned properly, - `drop_contributor_and_reassign_tasks_test`
        as well as a replacement contributor has the right amount of tasks assigned

*/

/// Drops a contributor who does not affect other contributors or verifiers.
fn coordinator_drop_contributor_basic_test() -> anyhow::Result<()> {
    let parameters = Parameters::Custom((
        ContributionMode::Chunked,
        ProvingSystem::Groth16,
        CurveKind::Bls12_377,
        6,  /* power */
        16, /* batch_size */
        16, /* chunk_size */
    ));
    let environment = initialize_test_environment_with_debug(&Testing::from(parameters).into());
    let number_of_chunks = environment.number_of_chunks() as usize;

    // Instantiate a coordinator.
    let coordinator = Coordinator::new(environment, Box::new(Dummy))?;

    // Initialize the ceremony to round 0.
    coordinator.initialize()?;
    assert_eq!(0, coordinator.current_round_height()?);

    // Add a contributor and verifier to the queue.
    let (contributor1, contributor_signing_key1, seed1) = create_contributor("1");
    let (contributor2, contributor_signing_key2, seed2) = create_contributor("2");
    let (verifier, verifier_signing_key) = create_verifier("1");
    coordinator.add_to_queue(contributor1.clone(), 10)?;
    coordinator.add_to_queue(contributor2.clone(), 9)?;
    coordinator.add_to_queue(verifier.clone(), 10)?;
    assert_eq!(2, coordinator.number_of_queue_contributors());
    assert_eq!(1, coordinator.number_of_queue_verifiers());
    assert!(coordinator.is_queue_contributor(&contributor1));
    assert!(coordinator.is_queue_contributor(&contributor2));
    assert!(coordinator.is_queue_verifier(&verifier));
    assert!(!coordinator.is_current_contributor(&contributor1));
    assert!(!coordinator.is_current_contributor(&contributor2));
    assert!(!coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Update the ceremony to round 1.
    coordinator.update()?;
    assert_eq!(1, coordinator.current_round_height()?);
    assert_eq!(0, coordinator.number_of_queue_contributors());
    assert_eq!(0, coordinator.number_of_queue_verifiers());
    assert!(!coordinator.is_queue_contributor(&contributor1));
    assert!(!coordinator.is_queue_contributor(&contributor2));
    assert!(!coordinator.is_queue_verifier(&verifier));
    assert!(coordinator.is_current_contributor(&contributor1));
    assert!(coordinator.is_current_contributor(&contributor2));
    assert!(coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Contribute and verify up to the penultimate chunk.
    for _ in 0..(number_of_chunks - 1) {
        coordinator.contribute(&contributor1, &contributor_signing_key1, &seed1)?;
        coordinator.contribute(&contributor2, &contributor_signing_key2, &seed2)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
    }
    assert!(!coordinator.is_queue_contributor(&contributor1));
    assert!(!coordinator.is_queue_contributor(&contributor2));
    assert!(!coordinator.is_queue_verifier(&verifier));
    assert!(coordinator.is_current_contributor(&contributor1));
    assert!(coordinator.is_current_contributor(&contributor2));
    assert!(coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Drop the contributor from the current round.
    let locators = coordinator.drop_participant(&contributor1)?;
    assert_eq!(&number_of_chunks - 1, locators.len());
    assert!(!coordinator.is_queue_contributor(&contributor1));
    assert!(!coordinator.is_queue_contributor(&contributor2));
    assert!(!coordinator.is_queue_verifier(&verifier));
    assert!(!coordinator.is_current_contributor(&contributor1));
    assert!(coordinator.is_current_contributor(&contributor2));
    assert!(coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Check that contributor 1 was dropped and coordinator state was updated.
    let contributors = coordinator.current_contributors();
    assert_eq!(2, contributors.len());
    assert_eq!(0, contributors.par_iter().filter(|(p, _)| *p == contributor1).count());
    for (contributor, contributor_info) in contributors {
        if contributor == contributor2 {
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(4, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(4, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(3, contributor_info.disposed_tasks().len());
        } else {
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(8, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(0, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(0, contributor_info.disposed_tasks().len());
        }
    }

    // Print the coordinator state.
    let state = coordinator.state();
    debug!("{}", serde_json::to_string_pretty(&state)?);
    assert_eq!(1, state.current_round_height());

    debug!("{}", serde_json::to_string_pretty(&coordinator.current_round()?)?);

    Ok(())
}

/// Drops a contributor in between two contributors.
fn coordinator_drop_contributor_in_between_two_contributors_test() -> anyhow::Result<()> {
    let parameters = Parameters::Custom((
        ContributionMode::Chunked,
        ProvingSystem::Groth16,
        CurveKind::Bls12_377,
        6,  /* power */
        16, /* batch_size */
        16, /* chunk_size */
    ));
    let environment = initialize_test_environment_with_debug(&Testing::from(parameters).into());
    let number_of_chunks = environment.number_of_chunks() as usize;

    // Instantiate a coordinator.
    let coordinator = Coordinator::new(environment.clone(), Box::new(Dummy))?;

    // Initialize the ceremony to round 0.
    coordinator.initialize()?;
    assert_eq!(0, coordinator.current_round_height()?);

    // Add a contributor and verifier to the queue.
    let (contributor1, contributor_signing_key1, seed1) = create_contributor("1");
    let (contributor2, contributor_signing_key2, seed2) = create_contributor("2");
    let (contributor3, contributor_signing_key3, seed3) = create_contributor("3");
    let (verifier, verifier_signing_key) = create_verifier("1");
    coordinator.add_to_queue(contributor1.clone(), 10)?;
    coordinator.add_to_queue(contributor2.clone(), 9)?;
    coordinator.add_to_queue(contributor3.clone(), 8)?;
    coordinator.add_to_queue(verifier.clone(), 10)?;
    assert_eq!(3, coordinator.number_of_queue_contributors());
    assert_eq!(1, coordinator.number_of_queue_verifiers());

    // Update the ceremony to round 1.
    coordinator.update()?;
    assert_eq!(1, coordinator.current_round_height()?);
    assert_eq!(0, coordinator.number_of_queue_contributors());
    assert_eq!(0, coordinator.number_of_queue_verifiers());

    // Contribute and verify up to the penultimate chunk.
    for _ in 0..(number_of_chunks - 1) {
        coordinator.contribute(&contributor1, &contributor_signing_key1, &seed1)?;
        coordinator.contribute(&contributor2, &contributor_signing_key2, &seed2)?;
        coordinator.contribute(&contributor3, &contributor_signing_key3, &seed3)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
    }
    assert!(!coordinator.is_queue_contributor(&contributor1));
    assert!(!coordinator.is_queue_contributor(&contributor2));
    assert!(!coordinator.is_queue_contributor(&contributor3));
    assert!(!coordinator.is_queue_verifier(&verifier));
    assert!(coordinator.is_current_contributor(&contributor1));
    assert!(coordinator.is_current_contributor(&contributor2));
    assert!(coordinator.is_current_contributor(&contributor3));
    assert!(coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_contributor(&contributor3));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Drop the contributor from the current round.
    let locators = coordinator.drop_participant(&contributor2)?;
    assert_eq!(&number_of_chunks - 1, locators.len());
    assert!(!coordinator.is_queue_contributor(&contributor1));
    assert!(!coordinator.is_queue_contributor(&contributor2));
    assert!(!coordinator.is_queue_contributor(&contributor3));
    assert!(!coordinator.is_queue_verifier(&verifier));
    assert!(coordinator.is_current_contributor(&contributor1));
    assert!(!coordinator.is_current_contributor(&contributor2));
    assert!(coordinator.is_current_contributor(&contributor3));
    assert!(coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_contributor(&contributor3));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Print the coordinator state.
    let state = coordinator.state();
    debug!("{}", serde_json::to_string_pretty(&state)?);
    assert_eq!(1, state.current_round_height());

    // Check that contributor 2 was dropped and coordinator state was updated.
    let contributors = coordinator.current_contributors();
    assert_eq!(3, contributors.len());
    assert_eq!(0, contributors.par_iter().filter(|(p, _)| *p == contributor2).count());
    let mut tasks: HashSet<Task> = HashSet::new();
    for (contributor, contributor_info) in contributors {
        if contributor == contributor1 {
            tasks.extend(contributor_info.assigned_tasks().iter());
            tasks.extend(contributor_info.completed_tasks().iter());
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(6, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(2, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(5, contributor_info.disposed_tasks().len());
        } else if contributor == contributor3 {
            tasks.extend(contributor_info.assigned_tasks().iter());
            tasks.extend(contributor_info.completed_tasks().iter());
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(2, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(6, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(1, contributor_info.disposed_tasks().len());
        } else {
            tasks.extend(contributor_info.assigned_tasks().iter());
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(8, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(0, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(0, contributor_info.disposed_tasks().len());
        }
    }

    // Check that all tasks are present.
    assert_eq!(24, tasks.len());
    for chunk_id in 0..environment.number_of_chunks() {
        for contribution_id in 1..4 {
            debug!("Checking {:?}", Task::new(chunk_id, contribution_id));
            assert!(tasks.contains(&Task::new(chunk_id, contribution_id)));
        }
    }

    Ok(())
}

/// Drops a contributor with other contributors in pending tasks.
fn coordinator_drop_contributor_with_contributors_in_pending_tasks_test() -> anyhow::Result<()> {
    let parameters = Parameters::Custom((
        ContributionMode::Chunked,
        ProvingSystem::Groth16,
        CurveKind::Bls12_377,
        6,  /* power */
        16, /* batch_size */
        16, /* chunk_size */
    ));
    let environment = initialize_test_environment_with_debug(&Testing::from(parameters).into());
    let number_of_chunks = environment.number_of_chunks() as usize;

    // Instantiate a coordinator.
    let coordinator = Coordinator::new(environment.clone(), Box::new(Dummy))?;

    // Initialize the ceremony to round 0.
    coordinator.initialize()?;
    assert_eq!(0, coordinator.current_round_height()?);

    // Add a contributor and verifier to the queue.
    let (contributor1, contributor_signing_key1, seed1) = create_contributor("1");
    let (contributor2, contributor_signing_key2, seed2) = create_contributor("2");
    let (contributor3, contributor_signing_key3, seed3) = create_contributor("3");
    let (verifier, verifier_signing_key) = create_verifier("1");
    coordinator.add_to_queue(contributor1.clone(), 10)?;
    coordinator.add_to_queue(contributor2.clone(), 9)?;
    coordinator.add_to_queue(contributor3.clone(), 8)?;
    coordinator.add_to_queue(verifier.clone(), 10)?;
    assert_eq!(3, coordinator.number_of_queue_contributors());
    assert_eq!(1, coordinator.number_of_queue_verifiers());

    // Update the ceremony to round 1.
    coordinator.update()?;
    assert_eq!(1, coordinator.current_round_height()?);
    assert_eq!(0, coordinator.number_of_queue_contributors());
    assert_eq!(0, coordinator.number_of_queue_verifiers());

    // Contribute and verify up to 2 before the final chunk.
    for _ in 0..(number_of_chunks - 2) {
        coordinator.contribute(&contributor1, &contributor_signing_key1, &seed1)?;
        coordinator.contribute(&contributor2, &contributor_signing_key2, &seed2)?;
        coordinator.contribute(&contributor3, &contributor_signing_key3, &seed3)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
    }

    // Lock the next task for contributor 1 and 3.
    coordinator.try_lock(&contributor1)?;
    coordinator.try_lock(&contributor3)?;

    // Check that coordinator state includes a pending task for contributor 1 and 3.
    let contributors = coordinator.current_contributors();
    assert_eq!(3, contributors.len());
    assert_eq!(1, contributors.par_iter().filter(|(p, _)| *p == contributor2).count());
    let mut tasks: HashSet<Task> = HashSet::new();
    for (contributor, contributor_info) in contributors {
        if contributor == contributor1 || contributor == contributor3 {
            tasks.extend(contributor_info.assigned_tasks().iter());
            tasks.extend(contributor_info.pending_tasks().iter());
            tasks.extend(contributor_info.completed_tasks().iter());
            assert_eq!(1, contributor_info.locked_chunks().len());
            assert_eq!(1, contributor_info.assigned_tasks().len());
            assert_eq!(1, contributor_info.pending_tasks().len());
            assert_eq!(6, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(0, contributor_info.disposed_tasks().len());
        } else {
            tasks.extend(contributor_info.assigned_tasks().iter());
            tasks.extend(contributor_info.completed_tasks().iter());
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(2, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(6, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(0, contributor_info.disposed_tasks().len());
        }
    }

    // Check that all tasks are present.
    assert_eq!(24, tasks.len());
    for chunk_id in 0..environment.number_of_chunks() {
        for contribution_id in 1..4 {
            debug!("Checking {:?}", Task::new(chunk_id, contribution_id));
            assert!(tasks.contains(&Task::new(chunk_id, contribution_id)));
        }
    }

    // Drop the contributor from the current round.
    let locators = coordinator.drop_participant(&contributor2)?;
    assert_eq!(&number_of_chunks - 2, locators.len());
    assert!(!coordinator.is_queue_contributor(&contributor1));
    assert!(!coordinator.is_queue_contributor(&contributor2));
    assert!(!coordinator.is_queue_contributor(&contributor3));
    assert!(!coordinator.is_queue_verifier(&verifier));
    assert!(coordinator.is_current_contributor(&contributor1));
    assert!(!coordinator.is_current_contributor(&contributor2));
    assert!(coordinator.is_current_contributor(&contributor3));
    assert!(coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_contributor(&contributor3));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Print the coordinator state.
    let state = coordinator.state();
    debug!("{}", serde_json::to_string_pretty(&state)?);
    assert_eq!(1, state.current_round_height());

    // Check that contributor 2 was dropped and coordinator state was updated.
    let contributors = coordinator.current_contributors();
    assert_eq!(3, contributors.len());
    assert_eq!(0, contributors.par_iter().filter(|(p, _)| *p == contributor2).count());
    let mut tasks: HashSet<Task> = HashSet::new();
    for (contributor, contributor_info) in contributors {
        if contributor == contributor1 {
            tasks.extend(contributor_info.assigned_tasks().iter());
            tasks.extend(contributor_info.completed_tasks().iter());
            assert_eq!(1, contributor_info.locked_chunks().len());
            assert_eq!(6, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(2, contributor_info.completed_tasks().len());
            assert_eq!(1, contributor_info.disposing_tasks().len());
            assert_eq!(4, contributor_info.disposed_tasks().len());
        } else if contributor == contributor3 {
            tasks.extend(contributor_info.assigned_tasks().iter());
            tasks.extend(contributor_info.pending_tasks().iter());
            tasks.extend(contributor_info.completed_tasks().iter());
            assert_eq!(1, contributor_info.locked_chunks().len());
            assert_eq!(2, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(6, contributor_info.completed_tasks().len());
            assert_eq!(1, contributor_info.disposing_tasks().len());
            assert_eq!(0, contributor_info.disposed_tasks().len());
        } else {
            tasks.extend(contributor_info.assigned_tasks().iter());
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(8, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(0, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(0, contributor_info.disposed_tasks().len());
        }
    }

    // Check that all tasks are present.
    assert_eq!(24, tasks.len());
    for chunk_id in 0..environment.number_of_chunks() {
        for contribution_id in 1..4 {
            debug!("Checking {:?}", Task::new(chunk_id, contribution_id));
            assert!(tasks.contains(&Task::new(chunk_id, contribution_id)));
        }
    }

    Ok(())
}

/// Drops a contributor with locked chunks and other contributors in pending tasks.
fn coordinator_drop_contributor_locked_chunks_test() -> anyhow::Result<()> {
    let parameters = Parameters::Custom((
        ContributionMode::Chunked,
        ProvingSystem::Groth16,
        CurveKind::Bls12_377,
        6,  /* power */
        16, /* batch_size */
        16, /* chunk_size */
    ));
    let environment = initialize_test_environment_with_debug(&Testing::from(parameters).into());
    let number_of_chunks = environment.number_of_chunks() as usize;

    // Instantiate a coordinator.
    let coordinator = Coordinator::new(environment.clone(), Box::new(Dummy))?;

    // Initialize the ceremony to round 0.
    coordinator.initialize()?;
    assert_eq!(0, coordinator.current_round_height()?);

    // Add a contributor and verifier to the queue.
    let (contributor1, contributor_signing_key1, seed1) = create_contributor("1");
    let (contributor2, contributor_signing_key2, seed2) = create_contributor("2");
    let (contributor3, contributor_signing_key3, seed3) = create_contributor("3");
    let (verifier, verifier_signing_key) = create_verifier("1");
    coordinator.add_to_queue(contributor1.clone(), 10)?;
    coordinator.add_to_queue(contributor2.clone(), 9)?;
    coordinator.add_to_queue(contributor3.clone(), 8)?;
    coordinator.add_to_queue(verifier.clone(), 10)?;
    assert_eq!(3, coordinator.number_of_queue_contributors());
    assert_eq!(1, coordinator.number_of_queue_verifiers());

    // Update the ceremony to round 1.
    coordinator.update()?;
    assert_eq!(1, coordinator.current_round_height()?);
    assert_eq!(0, coordinator.number_of_queue_contributors());
    assert_eq!(0, coordinator.number_of_queue_verifiers());

    // Contribute and verify up to 2 before the final chunk.
    for _ in 0..(number_of_chunks - 2) {
        coordinator.contribute(&contributor1, &contributor_signing_key1, &seed1)?;
        coordinator.contribute(&contributor2, &contributor_signing_key2, &seed2)?;
        coordinator.contribute(&contributor3, &contributor_signing_key3, &seed3)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
    }

    // Lock the next task for contributor 1 and 3.
    coordinator.try_lock(&contributor1)?;
    coordinator.try_lock(&contributor3)?;

    // Check that coordinator state includes a pending task for contributor 1 and 3.
    let contributors = coordinator.current_contributors();
    assert_eq!(3, contributors.len());
    assert_eq!(1, contributors.par_iter().filter(|(p, _)| *p == contributor2).count());
    let mut tasks: HashSet<Task> = HashSet::new();
    for (contributor, contributor_info) in contributors {
        if contributor == contributor1 || contributor == contributor3 {
            tasks.extend(contributor_info.assigned_tasks().iter());
            tasks.extend(contributor_info.pending_tasks().iter());
            tasks.extend(contributor_info.completed_tasks().iter());
            assert_eq!(1, contributor_info.locked_chunks().len());
            assert_eq!(1, contributor_info.assigned_tasks().len());
            assert_eq!(1, contributor_info.pending_tasks().len());
            assert_eq!(6, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(0, contributor_info.disposed_tasks().len());
        } else {
            tasks.extend(contributor_info.assigned_tasks().iter());
            tasks.extend(contributor_info.completed_tasks().iter());
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(2, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(6, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(0, contributor_info.disposed_tasks().len());
        }
    }

    // Check that all tasks are present.
    assert_eq!(24, tasks.len());
    for chunk_id in 0..environment.number_of_chunks() {
        for contribution_id in 1..4 {
            debug!("Checking {:?}", Task::new(chunk_id, contribution_id));
            assert!(tasks.contains(&Task::new(chunk_id, contribution_id)));
        }
    }

    // Lock the next task for contributor 2.
    coordinator.try_lock(&contributor2)?;

    // Drop the contributor from the current round.
    let locators = coordinator.drop_participant(&contributor2)?;
    assert_eq!(&number_of_chunks - 2, locators.len());
    assert!(!coordinator.is_queue_contributor(&contributor1));
    assert!(!coordinator.is_queue_contributor(&contributor2));
    assert!(!coordinator.is_queue_contributor(&contributor3));
    assert!(!coordinator.is_queue_verifier(&verifier));
    assert!(coordinator.is_current_contributor(&contributor1));
    assert!(!coordinator.is_current_contributor(&contributor2));
    assert!(coordinator.is_current_contributor(&contributor3));
    assert!(coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_contributor(&contributor3));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Print the coordinator state.
    let state = coordinator.state();
    debug!("{}", serde_json::to_string_pretty(&state)?);
    assert_eq!(1, state.current_round_height());

    // Check that contributor 2 was dropped and coordinator state was updated.
    let contributors = coordinator.current_contributors();
    assert_eq!(3, contributors.len());
    assert_eq!(0, contributors.par_iter().filter(|(p, _)| *p == contributor2).count());
    let mut tasks: HashSet<Task> = HashSet::new();
    for (contributor, contributor_info) in contributors {
        if contributor == contributor1 {
            tasks.extend(contributor_info.assigned_tasks().iter());
            tasks.extend(contributor_info.completed_tasks().iter());
            assert_eq!(1, contributor_info.locked_chunks().len());
            assert_eq!(6, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(2, contributor_info.completed_tasks().len());
            assert_eq!(1, contributor_info.disposing_tasks().len());
            assert_eq!(4, contributor_info.disposed_tasks().len());
        } else if contributor == contributor3 {
            tasks.extend(contributor_info.assigned_tasks().iter());
            tasks.extend(contributor_info.pending_tasks().iter());
            tasks.extend(contributor_info.completed_tasks().iter());
            assert_eq!(1, contributor_info.locked_chunks().len());
            assert_eq!(2, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(6, contributor_info.completed_tasks().len());
            assert_eq!(1, contributor_info.disposing_tasks().len());
            assert_eq!(0, contributor_info.disposed_tasks().len());
        } else {
            tasks.extend(contributor_info.assigned_tasks().iter());
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(8, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(0, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(0, contributor_info.disposed_tasks().len());
        }
    }

    // Check that all tasks are present.
    assert_eq!(24, tasks.len());
    for chunk_id in 0..environment.number_of_chunks() {
        for contribution_id in 1..4 {
            debug!("Checking {:?}", Task::new(chunk_id, contribution_id));
            assert!(tasks.contains(&Task::new(chunk_id, contribution_id)));
        }
    }

    // Print the current round of the ceremony.
    debug!("{}", serde_json::to_string_pretty(&coordinator.current_round()?)?);

    Ok(())
}

/// Drops a contributor and removes all contributions from the contributor.
fn coordinator_drop_contributor_removes_contributions() -> anyhow::Result<()> {
    let parameters = Parameters::Custom((
        ContributionMode::Chunked,
        ProvingSystem::Groth16,
        CurveKind::Bls12_377,
        6,  /* power */
        16, /* batch_size */
        16, /* chunk_size */
    ));
    let environment = initialize_test_environment_with_debug(&Testing::from(parameters).into());
    let number_of_chunks = environment.number_of_chunks() as usize;

    // Instantiate a coordinator.
    let coordinator = Coordinator::new(environment, Box::new(Dummy))?;

    // Initialize the ceremony to round 0.
    coordinator.initialize()?;
    assert_eq!(0, coordinator.current_round_height()?);

    // Add a contributor and verifier to the queue.
    let (contributor1, contributor_signing_key1, seed1) = create_contributor("1");
    let (contributor2, contributor_signing_key2, seed2) = create_contributor("2");
    let (verifier, verifier_signing_key) = create_verifier("1");
    coordinator.add_to_queue(contributor1.clone(), 10)?;
    coordinator.add_to_queue(contributor2.clone(), 9)?;
    coordinator.add_to_queue(verifier.clone(), 10)?;
    assert_eq!(2, coordinator.number_of_queue_contributors());
    assert_eq!(1, coordinator.number_of_queue_verifiers());
    assert!(coordinator.is_queue_contributor(&contributor1));
    assert!(coordinator.is_queue_contributor(&contributor2));
    assert!(coordinator.is_queue_verifier(&verifier));
    assert!(!coordinator.is_current_contributor(&contributor1));
    assert!(!coordinator.is_current_contributor(&contributor2));
    assert!(!coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Update the ceremony to round 1.
    coordinator.update()?;
    assert_eq!(1, coordinator.current_round_height()?);
    assert_eq!(0, coordinator.number_of_queue_contributors());
    assert_eq!(0, coordinator.number_of_queue_verifiers());
    assert!(!coordinator.is_queue_contributor(&contributor1));
    assert!(!coordinator.is_queue_contributor(&contributor2));
    assert!(!coordinator.is_queue_verifier(&verifier));
    assert!(coordinator.is_current_contributor(&contributor1));
    assert!(coordinator.is_current_contributor(&contributor2));
    assert!(coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Contribute and verify up to the penultimate chunk.
    for _ in 0..(number_of_chunks - 1) {
        coordinator.contribute(&contributor1, &contributor_signing_key1, &seed1)?;
        coordinator.contribute(&contributor2, &contributor_signing_key2, &seed2)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
    }
    assert!(!coordinator.is_queue_contributor(&contributor1));
    assert!(!coordinator.is_queue_contributor(&contributor2));
    assert!(!coordinator.is_queue_verifier(&verifier));
    assert!(coordinator.is_current_contributor(&contributor1));
    assert!(coordinator.is_current_contributor(&contributor2));
    assert!(coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Drop the contributor from the current round.
    let locators = coordinator.drop_participant(&contributor1)?;
    assert_eq!(&number_of_chunks - 1, locators.len());
    assert!(!coordinator.is_queue_contributor(&contributor1));
    assert!(!coordinator.is_queue_contributor(&contributor2));
    assert!(!coordinator.is_queue_verifier(&verifier));
    assert!(!coordinator.is_current_contributor(&contributor1));
    assert!(coordinator.is_current_contributor(&contributor2));
    assert!(coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Check that contributor 1 was dropped and coordinator state was updated.
    let contributors = coordinator.current_contributors();
    assert_eq!(2, contributors.len());
    assert_eq!(0, contributors.par_iter().filter(|(p, _)| *p == contributor1).count());
    for (contributor, contributor_info) in contributors {
        if contributor == contributor2 {
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(4, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(4, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(3, contributor_info.disposed_tasks().len());
        } else {
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(8, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(0, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(0, contributor_info.disposed_tasks().len());
        }
    }

    for chunk in coordinator.current_round()?.chunks() {
        let num_contributor1_chunk_contributions = chunk
            .get_contributions()
            .par_iter()
            .filter(|(_, contribution)| contribution.get_contributor() == &Some(contributor1.clone()))
            .count();

        assert_eq!(num_contributor1_chunk_contributions, 0);
    }

    // Print the coordinator state.
    let state = coordinator.state();
    debug!("{}", serde_json::to_string_pretty(&state)?);
    assert_eq!(1, state.current_round_height());

    // Print the current round of the ceremony.
    debug!("{}", serde_json::to_string_pretty(&coordinator.current_round()?)?);

    Ok(())
}

/// Drops a contributor and removes all subsequent contributions.
fn coordinator_drop_contributor_removes_subsequent_contributions() -> anyhow::Result<()> {
    // TODO (raychu86): Implement this test.

    Ok(())
}

/// Drops a contributor and clears locks for contributors/verifiers working on disposed tasks.
fn coordinator_drop_contributor_clear_locks_test() -> anyhow::Result<()> {
    let parameters = Parameters::Custom((
        ContributionMode::Chunked,
        ProvingSystem::Groth16,
        CurveKind::Bls12_377,
        6,  /* power */
        16, /* batch_size */
        16, /* chunk_size */
    ));
    let environment = initialize_test_environment_with_debug(&Testing::from(parameters).into());
    let number_of_chunks = environment.number_of_chunks() as usize;

    // Instantiate a coordinator.
    let coordinator = Coordinator::new(environment.clone(), Box::new(Dummy))?;

    // Initialize the ceremony to round 0.
    coordinator.initialize()?;
    assert_eq!(0, coordinator.current_round_height()?);

    // Add a contributor and verifier to the queue.
    let (contributor1, contributor_signing_key1, seed1) = create_contributor("1");
    let (contributor2, contributor_signing_key2, seed2) = create_contributor("2");
    let (contributor3, contributor_signing_key3, seed3) = create_contributor("3");
    let (verifier, verifier_signing_key) = create_verifier("1");
    coordinator.add_to_queue(contributor1.clone(), 10)?;
    coordinator.add_to_queue(contributor2.clone(), 9)?;
    coordinator.add_to_queue(contributor3.clone(), 8)?;
    coordinator.add_to_queue(verifier.clone(), 10)?;
    assert_eq!(3, coordinator.number_of_queue_contributors());
    assert_eq!(1, coordinator.number_of_queue_verifiers());

    // Update the ceremony to round 1.
    coordinator.update()?;
    assert_eq!(1, coordinator.current_round_height()?);
    assert_eq!(0, coordinator.number_of_queue_contributors());
    assert_eq!(0, coordinator.number_of_queue_verifiers());

    // Contribute and verify up to 2 before the final chunk.
    for _ in 0..(number_of_chunks - 2) {
        coordinator.contribute(&contributor1, &contributor_signing_key1, &seed1)?;
        coordinator.contribute(&contributor2, &contributor_signing_key2, &seed2)?;
        coordinator.contribute(&contributor3, &contributor_signing_key3, &seed3)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
    }

    // Contribute up to the final chunk.
    coordinator.contribute(&contributor1, &contributor_signing_key1, &seed1)?;
    coordinator.contribute(&contributor2, &contributor_signing_key2, &seed2)?;
    coordinator.contribute(&contributor3, &contributor_signing_key3, &seed3)?;

    // Lock the next task for the verifier and contributor 1 and 3.
    let (contributor1_locked_chunk_id, _, _, _) = coordinator.try_lock(&contributor1)?;
    let (verifier_locked_chunk_id, _, _, _) = coordinator.try_lock(&verifier)?;

    // Print the coordinator state.
    let state = coordinator.state();
    debug!("{}", serde_json::to_string_pretty(&state)?);
    assert_eq!(1, state.current_round_height());

    // Check that coordinator state includes a pending task for contributor 1 and 3.
    let contributors = coordinator.current_contributors();
    assert_eq!(3, contributors.len());
    assert_eq!(1, contributors.par_iter().filter(|(p, _)| *p == contributor2).count());
    let mut tasks: HashSet<Task> = HashSet::new();
    for (contributor, contributor_info) in contributors {
        if contributor == contributor1 {
            tasks.extend(contributor_info.assigned_tasks().iter());
            tasks.extend(contributor_info.pending_tasks().iter());
            tasks.extend(contributor_info.completed_tasks().iter());
            assert_eq!(1, contributor_info.locked_chunks().len());
            assert_eq!(0, contributor_info.assigned_tasks().len());
            assert_eq!(1, contributor_info.pending_tasks().len());
            assert_eq!(7, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(0, contributor_info.disposed_tasks().len());
        } else if contributor == contributor3 {
            tasks.extend(contributor_info.assigned_tasks().iter());
            tasks.extend(contributor_info.pending_tasks().iter());
            tasks.extend(contributor_info.completed_tasks().iter());
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(1, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(7, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(0, contributor_info.disposed_tasks().len());
        } else {
            tasks.extend(contributor_info.assigned_tasks().iter());
            tasks.extend(contributor_info.completed_tasks().iter());
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(1, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(7, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(0, contributor_info.disposed_tasks().len());
        }
    }

    // Check that all tasks are present.
    assert_eq!(24, tasks.len());
    for chunk_id in 0..environment.number_of_chunks() {
        for contribution_id in 1..4 {
            debug!("Checking {:?}", Task::new(chunk_id, contribution_id));
            assert!(tasks.contains(&Task::new(chunk_id, contribution_id)));
        }
    }

    // Lock the next task for contributor 2.
    coordinator.try_lock(&contributor2)?;

    // Drop the contributor from the current round.
    let locators = coordinator.drop_participant(&contributor2)?;
    assert_eq!(&number_of_chunks - 1, locators.len());
    assert!(!coordinator.is_queue_contributor(&contributor1));
    assert!(!coordinator.is_queue_contributor(&contributor2));
    assert!(!coordinator.is_queue_contributor(&contributor3));
    assert!(!coordinator.is_queue_verifier(&verifier));
    assert!(coordinator.is_current_contributor(&contributor1));
    assert!(!coordinator.is_current_contributor(&contributor2));
    assert!(coordinator.is_current_contributor(&contributor3));
    assert!(coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_contributor(&contributor3));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Run `try_contribute` and `try_verify` to remove disposed tasks.
    coordinator.try_contribute(&contributor1, contributor1_locked_chunk_id)?;
    coordinator.try_verify(&verifier, verifier_locked_chunk_id)?;

    // Print the coordinator state.
    let state = coordinator.state();
    debug!("{}", serde_json::to_string_pretty(&state)?);
    assert_eq!(1, state.current_round_height());

    // Check that contributor 2 was dropped and coordinator state was updated.
    let contributors = coordinator.current_contributors();
    assert_eq!(3, contributors.len());
    assert_eq!(0, contributors.par_iter().filter(|(p, _)| *p == contributor2).count());
    let mut tasks: HashSet<Task> = HashSet::new();
    for (contributor, contributor_info) in contributors {
        if contributor == contributor1 {
            tasks.extend(contributor_info.assigned_tasks().iter());
            tasks.extend(contributor_info.completed_tasks().iter());
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(6, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(2, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(6, contributor_info.disposed_tasks().len());
        } else if contributor == contributor3 {
            tasks.extend(contributor_info.assigned_tasks().iter());
            tasks.extend(contributor_info.completed_tasks().iter());
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(2, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(6, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(1, contributor_info.disposed_tasks().len());
        } else {
            tasks.extend(contributor_info.assigned_tasks().iter());
            assert_eq!(0, contributor_info.locked_chunks().len());
            assert_eq!(8, contributor_info.assigned_tasks().len());
            assert_eq!(0, contributor_info.pending_tasks().len());
            assert_eq!(0, contributor_info.completed_tasks().len());
            assert_eq!(0, contributor_info.disposing_tasks().len());
            assert_eq!(0, contributor_info.disposed_tasks().len());
        }
    }

    // Check that all tasks are present.
    assert_eq!(24, tasks.len());
    for chunk_id in 0..environment.number_of_chunks() {
        for contribution_id in 1..4 {
            debug!("Checking {:?}", Task::new(chunk_id, contribution_id));
            assert!(tasks.contains(&Task::new(chunk_id, contribution_id)));
        }
    }

    let verifiers = coordinator.current_verifiers();
    assert_eq!(1, verifiers.len());
    for (_, verifier_info) in verifiers {
        assert_eq!(0, verifier_info.locked_chunks().len());
        assert_eq!(2, verifier_info.assigned_tasks().len());
        assert_eq!(0, verifier_info.pending_tasks().len());
        assert_eq!(18, verifier_info.completed_tasks().len());
        assert_eq!(0, verifier_info.disposing_tasks().len());
        assert_eq!(1, verifier_info.disposed_tasks().len());
    }

    Ok(())
}

/// Drops a multiple contributors an replaces with the coordinator contributor.
fn coordinator_drop_multiple_contributors_test() -> anyhow::Result<()> {
    let parameters = Parameters::Custom((
        ContributionMode::Chunked,
        ProvingSystem::Groth16,
        CurveKind::Bls12_377,
        6,  /* power */
        16, /* batch_size */
        16, /* chunk_size */
    ));
    let testing = Testing::from(parameters).coordinator_contributors(&[
        Participant::new_contributor("testing-coordinator-contributor-1"),
        Participant::new_contributor("testing-coordinator-contributor-2"),
        Participant::new_contributor("testing-coordinator-contributor-3"),
    ]);
    let environment = initialize_test_environment_with_debug(&testing.into());

    let number_of_chunks = environment.number_of_chunks() as usize;

    // Instantiate a coordinator.
    let coordinator = Coordinator::new(environment.clone(), Box::new(Dummy))?;

    // Initialize the ceremony to round 0.
    coordinator.initialize()?;
    assert_eq!(0, coordinator.current_round_height()?);

    // Add a contributor and verifier to the queue.
    let (contributor1, contributor_signing_key1, seed1) = create_contributor("1");
    let (contributor2, contributor_signing_key2, seed2) = create_contributor("2");
    let (contributor3, contributor_signing_key3, seed3) = create_contributor("3");
    let (verifier, verifier_signing_key) = create_verifier("1");
    coordinator.add_to_queue(contributor1.clone(), 10)?;
    coordinator.add_to_queue(contributor2.clone(), 9)?;
    coordinator.add_to_queue(contributor3.clone(), 8)?;
    coordinator.add_to_queue(verifier.clone(), 10)?;
    assert_eq!(3, coordinator.number_of_queue_contributors());
    assert_eq!(1, coordinator.number_of_queue_verifiers());

    // Update the ceremony to round 1.
    coordinator.update()?;
    assert_eq!(1, coordinator.current_round_height()?);
    assert_eq!(0, coordinator.number_of_queue_contributors());
    assert_eq!(0, coordinator.number_of_queue_verifiers());

    // Contribute and verify up to 2 before the final chunk.
    for _ in 0..(number_of_chunks - 2) {
        coordinator.contribute(&contributor1, &contributor_signing_key1, &seed1)?;
        coordinator.contribute(&contributor2, &contributor_signing_key2, &seed2)?;
        coordinator.contribute(&contributor3, &contributor_signing_key3, &seed3)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
    }

    // Aggregate all of the tasks for each of the contributors into a HashSet.
    let contributors = coordinator.current_contributors();
    assert_eq!(3, contributors.len());
    let mut tasks: HashSet<Task> = HashSet::new();
    for (_, contributor_info) in contributors {
        tasks.extend(contributor_info.assigned_tasks().iter());
        tasks.extend(contributor_info.completed_tasks().iter());
        assert_eq!(0, contributor_info.locked_chunks().len());
        assert_eq!(2, contributor_info.assigned_tasks().len());
        assert_eq!(0, contributor_info.pending_tasks().len());
        assert_eq!(6, contributor_info.completed_tasks().len());
        assert_eq!(0, contributor_info.disposing_tasks().len());
        assert_eq!(0, contributor_info.disposed_tasks().len());
    }

    // Check that all tasks are present.
    assert_eq!(24, tasks.len());
    for chunk_id in 0..environment.number_of_chunks() {
        for contribution_id in 1..4 {
            debug!("Checking {:?}", Task::new(chunk_id, contribution_id));
            assert!(tasks.contains(&Task::new(chunk_id, contribution_id)));
        }
    }

    // Lock the next tasks for contributor 1, 2, and 3.
    coordinator.try_lock(&contributor1)?;
    coordinator.try_lock(&contributor2)?;
    coordinator.try_lock(&contributor3)?;

    // Drop the contributor 1 from the current round.
    let locators = coordinator.drop_participant(&contributor1)?;
    assert_eq!(&number_of_chunks - 2, locators.len());
    assert!(!coordinator.is_queue_contributor(&contributor1));
    assert!(!coordinator.is_queue_contributor(&contributor2));
    assert!(!coordinator.is_queue_contributor(&contributor3));
    assert!(!coordinator.is_queue_verifier(&verifier));
    assert!(!coordinator.is_current_contributor(&contributor1));
    assert!(coordinator.is_current_contributor(&contributor2));
    assert!(coordinator.is_current_contributor(&contributor3));
    assert!(coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_contributor(&contributor3));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Drop the contributor 2 from the current round.
    let locators = coordinator.drop_participant(&contributor2)?;
    assert_eq!(&number_of_chunks - 2, locators.len());
    assert!(!coordinator.is_queue_contributor(&contributor1));
    assert!(!coordinator.is_queue_contributor(&contributor2));
    assert!(!coordinator.is_queue_contributor(&contributor3));
    assert!(!coordinator.is_queue_verifier(&verifier));
    assert!(!coordinator.is_current_contributor(&contributor1));
    assert!(!coordinator.is_current_contributor(&contributor2));
    assert!(coordinator.is_current_contributor(&contributor3));
    assert!(coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_contributor(&contributor3));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Drop the contributor 3 from the current round.
    let locators = coordinator.drop_participant(&contributor3)?;
    assert_eq!(&number_of_chunks - 4, locators.len());
    assert!(!coordinator.is_queue_contributor(&contributor1));
    assert!(!coordinator.is_queue_contributor(&contributor2));
    assert!(!coordinator.is_queue_contributor(&contributor3));
    assert!(!coordinator.is_queue_verifier(&verifier));
    assert!(!coordinator.is_current_contributor(&contributor1));
    assert!(!coordinator.is_current_contributor(&contributor2));
    assert!(!coordinator.is_current_contributor(&contributor3));
    assert!(coordinator.is_current_verifier(&verifier));
    assert!(!coordinator.is_finished_contributor(&contributor1));
    assert!(!coordinator.is_finished_contributor(&contributor2));
    assert!(!coordinator.is_finished_contributor(&contributor3));
    assert!(!coordinator.is_finished_verifier(&verifier));

    // Print the coordinator state.
    let state = coordinator.state();
    debug!("{}", serde_json::to_string_pretty(&state)?);
    assert_eq!(1, state.current_round_height());

    let contributors = coordinator.current_contributors();
    assert_eq!(3, contributors.len());
    assert_eq!(0, contributors.par_iter().filter(|(p, _)| *p == contributor1).count());
    assert_eq!(0, contributors.par_iter().filter(|(p, _)| *p == contributor2).count());
    assert_eq!(0, contributors.par_iter().filter(|(p, _)| *p == contributor3).count());

    // Aggregate all of the tasks for each of the contributors into a HashSet.
    let mut tasks: HashSet<Task> = HashSet::new();
    for (_, contributor_info) in contributors {
        tasks.extend(contributor_info.assigned_tasks().iter());
        assert_eq!(0, contributor_info.locked_chunks().len());
        assert_eq!(8, contributor_info.assigned_tasks().len());
        assert_eq!(0, contributor_info.pending_tasks().len());
        assert_eq!(0, contributor_info.completed_tasks().len());
        assert_eq!(0, contributor_info.disposing_tasks().len());
        assert_eq!(0, contributor_info.disposed_tasks().len());
    }

    // Check that all tasks are present.
    assert_eq!(24, tasks.len());
    for chunk_id in 0..environment.number_of_chunks() {
        for contribution_id in 1..4 {
            debug!("Checking {:?}", Task::new(chunk_id, contribution_id));
            assert!(tasks.contains(&Task::new(chunk_id, contribution_id)));
        }
    }

    Ok(())
}

fn try_lock_blocked_test() -> anyhow::Result<()> {
    let parameters = Parameters::Custom((
        ContributionMode::Chunked,
        ProvingSystem::Groth16,
        CurveKind::Bls12_377,
        7,  /* power */
        32, /* batch_size */
        32, /* chunk_size */
    ));
    let environment = initialize_test_environment_with_debug(&Testing::from(parameters).into());
    let number_of_chunks = environment.number_of_chunks() as usize;

    // Instantiate a coordinator.
    let coordinator = Coordinator::new(environment, Box::new(Dummy))?;

    // Initialize the ceremony to round 0.
    coordinator.initialize()?;
    assert_eq!(0, coordinator.current_round_height()?);

    // Meanwhile, add 2 contributors and 1 verifier to the queue.
    let (contributor1, contributor_signing_key1, seed1) = create_contributor("1");
    let (contributor2, contributor_signing_key2, seed2) = create_contributor("2");
    let (verifier, verifier_signing_key) = create_verifier("1");
    coordinator.add_to_queue(contributor1.clone(), 10)?;
    coordinator.add_to_queue(contributor2.clone(), 10)?;
    coordinator.add_to_queue(verifier.clone(), 10)?;
    assert_eq!(2, coordinator.number_of_queue_contributors());
    assert_eq!(1, coordinator.number_of_queue_verifiers());

    // Advance the ceremony from round 0 to round 1.
    coordinator.update()?;
    assert_eq!(1, coordinator.current_round_height()?);
    assert_eq!(0, coordinator.number_of_queue_contributors());
    assert_eq!(0, coordinator.number_of_queue_verifiers());

    // Fetch the bucket size.
    fn bucket_size(number_of_chunks: u64, number_of_contributors: u64) -> u64 {
        let number_of_buckets = number_of_contributors;
        let bucket_size = number_of_chunks / number_of_buckets;
        bucket_size
    }

    /*
     * |     BUCKET 0     |     BUCKET 1    |
     * |   0, 1, ...  m   |  m + 1, ... n   | <- Chunk IDs
     * |  ------------->  |  ------------>  |
     * |                  |  locked         | <- Contributor 2
     * |   done ... done  |  try_lock       | <- Contributor 1
     * |  ------------->  |  ------------>  |
     */

    // Lock first chunk for contributor 2.
    let (_, _, _, response) = coordinator.try_lock(&contributor2)?;

    // Run contributions for the first bucket as contributor 1.
    let bucket_size = bucket_size(number_of_chunks as u64, 2);
    for _ in 0..bucket_size {
        coordinator.contribute(&contributor1, &contributor_signing_key1, &seed1)?;
    }

    // Now try to lock the next chunk as contributor 1.
    //
    // This operation should be blocked by contributor 2,
    // who still holds the lock on this chunk.
    let result = coordinator.try_lock(&contributor1);
    assert!(result.is_err());

    // Run contribution on the locked chunk as contributor 2.
    {
        let (round_height, chunk_id, contribution_id, _) = coordinator.parse_contribution_file_locator(&response)?;
        coordinator.run_computation(
            round_height,
            chunk_id,
            contribution_id,
            &contributor2,
            &contributor_signing_key2,
            &seed2,
        )?;
        coordinator.try_contribute(&contributor2, chunk_id)?;
    }

    // Now try to lock the next chunk as contributor 1 again.
    //
    // This operation should be blocked by the verifier,
    // who needs to verify this chunk in order for contributor 1 to acquire the lock.
    let result = coordinator.try_lock(&contributor1);
    assert!(result.is_err());

    // Clear all pending verifications, so the locked chunk is released as well.
    while coordinator.verify(&verifier, &verifier_signing_key).is_ok() {}

    // Now try to lock the next chunk as contributor 1 again.
    //
    // This operation should no longer be blocked by contributor 2 or verifier,
    // who has released the lock on this chunk.
    let result = coordinator.try_lock(&contributor1);
    assert!(result.is_ok());

    Ok(())
}

/// Test that contributor/verifier `completed_tasks` are removed/moved
/// when a participant is dropped. Tests that the tasks declared in
/// the state file are updated correctly when that happens.
fn drop_contributor_state_file_updated_test() -> anyhow::Result<()> {
    let parameters = Parameters::Custom((
        ContributionMode::Chunked,
        ProvingSystem::Groth16,
        CurveKind::Bls12_377,
        6,  /* power */
        16, /* batch_size */
        16, /* chunk_size */
    ));
    let environment = initialize_test_environment_with_debug(&Testing::from(parameters).into());
    let number_of_chunks = environment.number_of_chunks() as usize;

    // Instantiate a coordinator.
    let coordinator = Coordinator::new(environment, Box::new(Dummy))?;

    // Initialize the ceremony to round 0.
    coordinator.initialize()?;

    let storage = coordinator.storage();

    {
        let read = storage.read().expect("unable to obtain read lock on storage");
        assert!(read.exists(&Locator::CoordinatorState));
        assert!(read.get(&Locator::CoordinatorState).is_ok());
    }

    // Add a contributor and verifier to the queue.
    let (contributor1, contributor_signing_key1, seed1) = create_contributor("1");
    let (contributor2, contributor_signing_key2, seed2) = create_contributor("2");
    let (verifier, verifier_signing_key) = create_verifier("1");
    coordinator.add_to_queue(contributor1.clone(), 10)?;
    coordinator.add_to_queue(contributor2.clone(), 9)?;
    coordinator.add_to_queue(verifier.clone(), 10)?;

    // Check state in the coordinator.
    {
        assert!(coordinator.current_contributor_info(&contributor1)?.is_none());
        assert!(coordinator.current_contributor_info(&contributor2)?.is_none());
        assert!(coordinator.current_verifier_info(&verifier)?.is_none());
    }

    // Check state in the storage.
    {
        let coordinator_state = match storage
            .read()
            .expect("unable to obtain read lock on storage")
            .get(&Locator::CoordinatorState)?
        {
            Object::CoordinatorState(state) => state,
            _ => panic!("unexpected object type"),
        };
        assert!(coordinator_state.current_contributor_info(&contributor1)?.is_none());
        assert!(coordinator_state.current_contributor_info(&contributor2)?.is_none());
        assert!(coordinator_state.current_verifier_info(&verifier)?.is_none());
    }

    // Update the ceremony to round 1.
    coordinator.update()?;

    let (contributor1_n_tasks, contributor2_n_tasks) = {
        let coordinator_state = match storage
            .read()
            .expect("unable to obtain read lock on storage")
            .get(&Locator::CoordinatorState)?
        {
            Object::CoordinatorState(state) => state,
            _ => panic!("unexpected object type"),
        };

        let contributor1_info = coordinator
            .current_contributor_info(&contributor1)?
            .expect("expected contributor1 to have info now that it is a current contributor in round 1");

        // Check the coordinator state matches what is in storage.
        assert_eq!(
            &contributor1_info,
            coordinator_state
                .current_contributor_info(&contributor1)?
                .expect("expected contributor1 to have info now that it is a current contributor in round 1")
        );

        assert!(!contributor1_info.assigned_tasks().is_empty());
        assert!(contributor1_info.pending_tasks().is_empty());
        assert!(contributor1_info.completed_tasks().is_empty());
        assert!(contributor1_info.disposing_tasks().is_empty());
        assert!(contributor1_info.disposed_tasks().is_empty());

        let contributor2_info = coordinator
            .current_contributor_info(&contributor2)?
            .expect("expected contributor2 to have info now that it is a current contributor in round 1");

        // Check the coordinator state matches what is in storage.
        assert_eq!(
            &contributor2_info,
            coordinator_state
                .current_contributor_info(&contributor2)?
                .expect("expected contributor2 to have info now that it is a current contributor in round 1")
        );

        assert!(!contributor2_info.assigned_tasks().is_empty());
        assert!(contributor2_info.pending_tasks().is_empty());
        assert!(contributor2_info.completed_tasks().is_empty());
        assert!(contributor2_info.disposing_tasks().is_empty());
        assert!(contributor2_info.disposed_tasks().is_empty());

        let verifier_info = coordinator
            .current_verifier_info(&verifier)?
            .expect("expected verifier to have info now that it is a verifier in round 1");

        // Check the coordinator state matches what is in storage.
        assert_eq!(
            &verifier_info,
            coordinator_state
                .current_verifier_info(&verifier)?
                .expect("expected verifier to have info now that it is a current verifier in round 1")
        );

        // No contributions yet, so nothing to verify.
        assert!(verifier_info.assigned_tasks().is_empty());
        assert!(verifier_info.pending_tasks().is_empty());
        assert!(verifier_info.completed_tasks().is_empty());
        assert!(verifier_info.disposing_tasks().is_empty());
        assert!(verifier_info.disposed_tasks().is_empty());

        (
            contributor1_info.assigned_tasks().len(),
            contributor2_info.assigned_tasks().len(),
        )
    };

    // Contribute and verify up to the penultimate chunk.
    for i in 0..(number_of_chunks - 1) {
        coordinator.contribute(&contributor1, &contributor_signing_key1, &seed1)?;
        coordinator.contribute(&contributor2, &contributor_signing_key2, &seed2)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;

        // Do some checks after the first chunk.
        if i == 0 {
            let coordinator_state = match storage
                .read()
                .expect("unable to obtain read lock on storage")
                .get(&Locator::CoordinatorState)?
            {
                Object::CoordinatorState(state) => state,
                _ => panic!("unexpected object type"),
            };

            let contributor1_info = coordinator
                .current_contributor_info(&contributor1)?
                .expect("expected contributor1 to have info now that it is a current contributor in round 1");

            // Check the coordinator state matches what is in storage.
            assert_eq!(
                &contributor1_info,
                coordinator_state
                    .current_contributor_info(&contributor1)?
                    .expect("expected contributor1 to have info now that it is a current contributor in round 1")
            );

            assert!(!contributor1_info.assigned_tasks().is_empty());
            assert!(contributor1_info.pending_tasks().is_empty());
            // Completed one contribution task.
            assert_eq!(1, contributor1_info.completed_tasks().len());
            assert!(contributor1_info.disposing_tasks().is_empty());
            assert!(contributor1_info.disposed_tasks().is_empty());

            let contributor2_info = coordinator
                .current_contributor_info(&contributor2)?
                .expect("expected contributor2 to have info now that it is a current contributor in round 1");

            // Check the coordinator state matches what is in storage.
            assert_eq!(
                &contributor2_info,
                coordinator_state
                    .current_contributor_info(&contributor2)?
                    .expect("expected contributor2 to have info now that it is a current contributor in round 1")
            );

            assert!(!contributor2_info.assigned_tasks().is_empty());
            assert!(contributor2_info.pending_tasks().is_empty());
            // Completed one contribution task.
            assert_eq!(1, contributor2_info.completed_tasks().len());
            assert!(contributor2_info.disposing_tasks().is_empty());
            assert!(contributor2_info.disposed_tasks().is_empty());

            let verifier_info = coordinator
                .current_verifier_info(&verifier)?
                .expect("expected verifier to have info now that it is a verifier in round 1");

            // Check the coordinator state matches what is in storage.
            assert_eq!(
                &verifier_info,
                coordinator_state
                    .current_verifier_info(&verifier)?
                    .expect("expected verifier to have info now that it is a current verifier in round 1")
            );

            // No contributions yet, so nothing to verify.
            assert!(verifier_info.assigned_tasks().is_empty());
            assert!(verifier_info.pending_tasks().is_empty());
            // Completed two verification tasks of the two contributions.
            assert_eq!(2, verifier_info.completed_tasks().len());
            assert!(verifier_info.disposing_tasks().is_empty());
            assert!(verifier_info.disposed_tasks().is_empty());
        }
    }

    {
        let coordinator_state = match storage
            .read()
            .expect("unable to obtain read lock on storage")
            .get(&Locator::CoordinatorState)?
        {
            Object::CoordinatorState(state) => state,
            _ => panic!("unexpected object type"),
        };

        let contributor1_info = coordinator
            .current_contributor_info(&contributor1)?
            .expect("expected contributor1 to have info now that it is a current contributor in round 1");

        // Check the coordinator state matches what is in storage.
        assert_eq!(
            &contributor1_info,
            coordinator_state
                .current_contributor_info(&contributor1)?
                .expect("expected contributor1 to have info now that it is a current contributor in round 1")
        );

        assert_eq!(1, contributor1_info.assigned_tasks().len());
        assert!(contributor1_info.pending_tasks().is_empty());
        assert_eq!(contributor1_n_tasks - 1, contributor1_info.completed_tasks().len());
        assert!(contributor1_info.disposing_tasks().is_empty());
        assert!(contributor1_info.disposed_tasks().is_empty());

        let contributor2_info = coordinator
            .current_contributor_info(&contributor2)?
            .expect("expected contributor2 to have info now that it is a current contributor in round 1");

        // Check the coordinator state matches what is in storage.
        assert_eq!(
            &contributor2_info,
            coordinator_state
                .current_contributor_info(&contributor2)?
                .expect("expected contributor2 to have info now that it is a current contributor in round 1")
        );

        assert_eq!(1, contributor2_info.assigned_tasks().len());
        assert!(contributor2_info.pending_tasks().is_empty());
        assert_eq!(contributor2_n_tasks - 1, contributor2_info.completed_tasks().len());
        assert!(contributor2_info.disposing_tasks().is_empty());
        assert!(contributor2_info.disposed_tasks().is_empty());

        let verifier_info = coordinator
            .current_verifier_info(&verifier)?
            .expect("expected verifier to have info now that it is a verifier in round 1");

        // Check the coordinator state matches what is in storage.
        assert_eq!(
            &verifier_info,
            coordinator_state
                .current_verifier_info(&verifier)?
                .expect("expected verifier to have info now that it is a current verifier in round 1")
        );

        assert!(verifier_info.pending_tasks().is_empty());
        assert!(!verifier_info.completed_tasks().is_empty());
        assert!(verifier_info.disposing_tasks().is_empty());
        assert!(verifier_info.disposed_tasks().is_empty());
    }

    assert!(coordinator.is_current_contributor(&contributor1));
    assert!(coordinator.is_current_contributor(&contributor2));
    assert!(coordinator.is_current_verifier(&verifier));

    // Drop the contributor from the current round.
    let locators = coordinator.drop_participant(&contributor1)?;
    assert_eq!(&number_of_chunks - 1, locators.len());

    assert!(!coordinator.is_current_contributor(&contributor1));
    assert!(coordinator.is_current_contributor(&contributor2));
    assert!(coordinator.is_current_verifier(&verifier));

    {
        let coordinator_state = match storage
            .read()
            .expect("unable to obtain read lock on storage")
            .get(&Locator::CoordinatorState)?
        {
            Object::CoordinatorState(state) => state,
            _ => panic!("unexpected object type"),
        };

        // There is now no current contributor1
        assert!(coordinator.current_contributor_info(&contributor1)?.is_none());

        // Check the coordinator state matches what is in storage.
        assert!(coordinator_state.current_contributor_info(&contributor1)?.is_none());

        let dropped_participant1_info = coordinator_state
            .dropped_contributors()
            .into_iter()
            .find(|contributor_info: &ParticipantInfo| contributor_info.participant() == contributor1)
            .expect("expected participant1 to be present in the list of dropped contributors");

        assert!(dropped_participant1_info.is_dropped());

        let contributor2_info = coordinator
            .current_contributor_info(&contributor2)?
            .expect("expected contributor2 to have info now that it is a current contributor in round 1");

        // Check the coordinator state matches what is in storage.
        assert_eq!(
            &contributor2_info,
            coordinator_state
                .current_contributor_info(&contributor2)?
                .expect("expected contributor2 to have info now that it is a current contributor in round 1")
        );

        // There are now more assigned tasks than before the participant drop.
        assert!(1 < contributor2_info.assigned_tasks().len());
        assert!(contributor2_info.pending_tasks().is_empty());
        // There are now less completed tasks than before the participant drop.
        assert!(contributor2_n_tasks - 1 > contributor2_info.completed_tasks().len());
        assert!(contributor2_info.disposing_tasks().is_empty());
        // Some of the tasks that depended on participant1's
        // contributions have been disposed.
        assert!(!contributor2_info.disposed_tasks().is_empty());

        let verifier_info = coordinator
            .current_verifier_info(&verifier)?
            .expect("expected verifier to have info now that it is a verifier in round 1");

        // Check the coordinator state matches what is in storage.
        assert_eq!(
            &verifier_info,
            coordinator_state
                .current_verifier_info(&verifier)?
                .expect("expected verifier to have info now that it is a current verifier in round 1")
        );

        assert!(verifier_info.pending_tasks().is_empty());
        assert!(!verifier_info.completed_tasks().is_empty());
        assert!(verifier_info.disposing_tasks().is_empty());
        // Some of the tasks that depended on participant1's
        // contributions have been disposed.
        assert!(!verifier_info.disposed_tasks().is_empty());
    }

    // Print the coordinator state.
    let state = coordinator.state();
    debug!("{}", serde_json::to_string_pretty(&state)?);
    assert_eq!(1, state.current_round_height());

    debug!("{}", serde_json::to_string_pretty(&coordinator.current_round()?)?);

    Ok(())
}

fn drop_contributor_and_reassign_tasks_test() -> anyhow::Result<()> {
    let parameters = Parameters::Custom((
        ContributionMode::Chunked,
        ProvingSystem::Groth16,
        CurveKind::Bls12_377,
        6,  /* power */
        16, /* batch_size */
        16, /* chunk_size */
    ));
    let environment = initialize_test_environment_with_debug(&Testing::from(parameters).into());
    let number_of_chunks = environment.number_of_chunks() as usize;

    // Instantiate a coordinator.
    let coordinator = Coordinator::new(environment, Box::new(Dummy))?;

    // Initialize the ceremony to round 0.
    coordinator.initialize()?;
    assert_eq!(0, coordinator.current_round_height()?);

    // Add a contributor and verifier to the queue.
    let (contributor1, contributor_signing_key1, seed1) = create_contributor("1");
    let (contributor2, contributor_signing_key2, seed2) = create_contributor("2");
    let (verifier, verifier_signing_key) = create_verifier("1");
    coordinator.add_to_queue(contributor1.clone(), 10)?;
    coordinator.add_to_queue(contributor2.clone(), 9)?;
    coordinator.add_to_queue(verifier.clone(), 10)?;

    // Update the ceremony to round 1.
    coordinator.update()?;

    for _ in 0..(number_of_chunks) {
        coordinator.contribute(&contributor1, &contributor_signing_key1, &seed1)?;
        coordinator.contribute(&contributor2, &contributor_signing_key2, &seed2)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
        coordinator.verify(&verifier, &verifier_signing_key)?;
    }

    for (_participant, contributor_info) in coordinator.current_contributors() {
        assert_eq!(contributor_info.completed_tasks().len(), 8);
        assert_eq!(contributor_info.assigned_tasks().len(), 0);
        assert_eq!(contributor_info.disposing_tasks().len(), 0);
        assert_eq!(contributor_info.disposed_tasks().len(), 0);
    }

    // Drop the contributor from the current round.
    let locators = coordinator.drop_participant(&contributor1)?;
    assert_eq!(number_of_chunks, locators.len());
    assert_eq!(false, coordinator.is_queue_contributor(&contributor1));
    assert_eq!(false, coordinator.is_queue_contributor(&contributor2));
    assert_eq!(false, coordinator.is_queue_verifier(&verifier));
    assert_eq!(false, coordinator.is_current_contributor(&contributor1));
    assert_eq!(true, coordinator.is_current_contributor(&contributor2));
    assert_eq!(true, coordinator.is_current_verifier(&verifier));
    assert_eq!(false, coordinator.is_finished_contributor(&contributor1));
    assert_eq!(false, coordinator.is_finished_contributor(&contributor2));
    assert_eq!(false, coordinator.is_finished_verifier(&verifier));

    for (participant, contributor_info) in coordinator.current_contributors() {
        if participant == contributor2 {
            assert_eq!(contributor_info.completed_tasks().len(), 4);
            assert_eq!(contributor_info.assigned_tasks().len(), 4);
            assert_eq!(contributor_info.disposing_tasks().len(), 0);
            assert_eq!(contributor_info.disposed_tasks().len(), 4);
        } else {
            // Replacement contributor
            assert_eq!(contributor_info.completed_tasks().len(), 0);
            assert_eq!(contributor_info.assigned_tasks().len(), 8);
            assert_eq!(contributor_info.disposing_tasks().len(), 0);
            assert_eq!(contributor_info.disposed_tasks().len(), 0);
        }
    }

    Ok(())
}

#[test]
#[serial]
fn test_round_on_groth16_bls12_377() {
    execute_round_test(ProvingSystem::Groth16, CurveKind::Bls12_377).unwrap();
}

#[test]
#[serial]
fn test_round_on_groth16_bw6_761() {
    execute_round_test(ProvingSystem::Groth16, CurveKind::BW6).unwrap();
}

#[test]
#[serial]
fn test_round_on_marlin_bls12_377() {
    execute_round_test(ProvingSystem::Marlin, CurveKind::Bls12_377).unwrap();
}

#[test]
#[named]
#[serial]
fn test_coordinator_drop_contributor_basic() {
    test_report!(coordinator_drop_contributor_basic_test);
}

#[test]
#[named]
#[serial]
fn test_coordinator_drop_contributor_in_between_two_contributors() {
    test_report!(coordinator_drop_contributor_in_between_two_contributors_test);
}

#[test]
#[named]
#[serial]
fn test_coordinator_drop_contributor_with_contributors_in_pending_tasks() {
    test_report!(coordinator_drop_contributor_with_contributors_in_pending_tasks_test);
}

#[test]
#[named]
#[serial]
fn test_coordinator_drop_contributor_with_locked_chunk() {
    test_report!(coordinator_drop_contributor_locked_chunks_test);
}

#[test]
#[named]
#[serial]
fn test_coordinator_drop_contributor_removes_contributions() {
    test_report!(coordinator_drop_contributor_removes_contributions);
}

#[test]
#[named]
#[serial]
fn test_coordinator_drop_contributor_removes_subsequent_contributions() {
    test_report!(coordinator_drop_contributor_removes_subsequent_contributions);
}

#[test]
#[named]
#[serial]
fn test_coordinator_drop_contributor_clear_locks() {
    test_report!(coordinator_drop_contributor_clear_locks_test);
}

#[test]
#[named]
#[serial]
fn test_coordinator_drop_multiple_contributors() {
    test_report!(coordinator_drop_multiple_contributors_test);
}

#[test]
#[named]
#[serial]
fn test_try_lock_blocked() {
    test_report!(try_lock_blocked_test);
}

#[test]
#[named]
#[serial]
fn test_drop_contributor_state_file_updated() {
    test_report!(drop_contributor_state_file_updated_test);
}

#[test]
#[named]
#[serial]
fn test_drop_contributor_and_reassign_tasks() {
    test_report!(drop_contributor_and_reassign_tasks_test);
}
