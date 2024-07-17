use std::sync::Arc;

use sov_mock_da::{MockAddress, MockBlock, MockBlockHeader, MockDaService};
use sov_rollup_interface::services::da::DaServiceWithRetries;
use sov_rollup_interface::zk::aggregated_proof::AggregatedProofPublicData;
use sov_stf_runner::InitVariant;
use tokio::task::JoinHandle;

use crate::helpers::runner_init::{initialize_runner, TestNode};

#[tokio::test(flavor = "multi_thread")]
async fn fetch_aggregated_proof_test() -> Result<(), anyhow::Error> {
    for jump in [1, 7] {
        let test_case = TestCase::new(jump);
        for nb_of_threads in [1, 3] {
            run_make_proof_sync(test_case, nb_of_threads).await?;
            run_make_proof_async(test_case, nb_of_threads).await?;
        }
    }

    Ok(())
}

// In this test, proofs are created just after batch is submitted to the DA.
async fn run_make_proof_sync(
    test_case: TestCase,
    nb_of_threads: usize,
) -> Result<(), anyhow::Error> {
    let tmpdir = tempfile::tempdir().unwrap();
    let jump = test_case.jump();

    let nb_of_batches = test_case.input.nb_of_batches;
    let (mut test_node, runner_task) = spawn(jump, nb_of_threads, tmpdir.path());

    for batch_number in 0..nb_of_batches {
        test_node.send_transaction().await.unwrap();
        test_node.make_block_proof();

        if (batch_number + 1) % jump == 0 {
            test_node.wait_for_aggregated_proof_posted_to_da().await?;
        }
    }

    test_node.try_send_aggregated_proof().await?;
    test_node.make_block_proof();

    let mut init_slot = 1;
    for _ in (0..nb_of_batches).step_by(jump) {
        let resp = test_node.wait_for_aggregated_proof_saved_in_db().await?;
        let pub_data = resp.proof.public_data();
        init_slot = calculate_and_check_slot_number(init_slot, jump, pub_data);
    }

    let public_data = test_node.get_latest_public_data().await?.unwrap();
    test_case.assert(&public_data);
    runner_task.abort();
    Ok(())
}

// In this test, proofs are created after multiple batches are submitted to the DA.
async fn run_make_proof_async(
    test_case: TestCase,
    nb_of_threads: usize,
) -> Result<(), anyhow::Error> {
    let tmpdir = tempfile::tempdir().unwrap();
    let jump = test_case.jump();
    let nb_of_batches = test_case.input.nb_of_batches;
    let (mut test_node, runner_task) = spawn(test_case.jump(), nb_of_threads, tmpdir.path());

    for _ in 0..nb_of_batches {
        test_node.send_transaction().await?;
    }

    for _ in 0..nb_of_batches {
        test_node.make_block_proof();
    }

    for _ in (0..nb_of_batches).step_by(jump) {
        test_node.wait_for_aggregated_proof_posted_to_da().await?;
    }

    test_node.try_send_aggregated_proof().await?;
    test_node.make_block_proof();

    let mut init_slot = 1;
    for _ in (0..nb_of_batches).step_by(jump) {
        let resp = test_node.wait_for_aggregated_proof_saved_in_db().await?;
        let pub_data = resp.proof.public_data();
        init_slot = calculate_and_check_slot_number(init_slot, jump, pub_data);
    }

    let public_data = test_node.get_latest_public_data().await?.unwrap();
    test_case.assert(&public_data);
    runner_task.abort();
    Ok(())
}

fn calculate_and_check_slot_number(
    init_slot: u64,
    jump: usize,
    pub_data: &AggregatedProofPublicData,
) -> u64 {
    assert_eq!(init_slot, pub_data.initial_slot_number);

    let final_slot = init_slot + jump as u64 - 1;
    assert_eq!(final_slot, pub_data.final_slot_number);

    final_slot + 1
}

fn spawn(
    jump: usize,
    nb_of_threads: usize,
    path: impl AsRef<std::path::Path>,
) -> (TestNode, JoinHandle<()>) {
    let genesis_block = MockBlock {
        header: MockBlockHeader::from_height(0),
        validity_cond: Default::default(),
        batch_blobs: vec![],
        proof_blobs: vec![],
    };
    let init_variant = InitVariant::Genesis {
        block: genesis_block,
        genesis_params: vec![1],
    };

    let da_service = Arc::new(DaServiceWithRetries::new_fast(MockDaService::new(
        MockAddress::new([11u8; 32]),
    )));

    let (mut runner, test_node) = initialize_runner(
        da_service,
        path.as_ref(),
        init_variant,
        jump,
        Some(nb_of_threads),
    );

    let join_handle = tokio::spawn(async move {
        runner.run_in_process().await.unwrap();
    });

    (test_node, join_handle)
}

#[derive(Clone, Copy)]
struct Input {
    jump: usize,
    nb_of_batches: usize,
}

#[derive(Clone, Copy)]
struct Output {
    initial_slot_number: u64,
}

#[derive(Clone, Copy)]
struct TestCase {
    input: Input,
    output: Output,
}

impl TestCase {
    fn new(jump: usize) -> Self {
        // Generate 7 aggregate-proofs worth of blocks
        let nb_of_batches = 7 * jump;
        // The initial slot number of the final proof.
        // The first proof covers slots 1..=jump, the second jump+1..=(2*jump), etc.
        let initial_slot_number = (6 * jump + 1) as u64;
        Self {
            input: Input {
                jump,
                nb_of_batches,
            },
            output: Output {
                initial_slot_number,
            },
        }
    }

    fn jump(&self) -> usize {
        self.input.jump
    }

    fn final_slot_number(&self) -> u64 {
        self.output.initial_slot_number + (self.input.jump as u64) - 1
    }

    fn assert(&self, public_data: &AggregatedProofPublicData) {
        assert_eq!(
            self.output.initial_slot_number,
            public_data.initial_slot_number,
        );

        assert_eq!(self.final_slot_number(), public_data.final_slot_number);
    }
}
