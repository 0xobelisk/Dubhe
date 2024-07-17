use std::collections::HashMap;
use std::convert::Infallible;

use borsh::BorshDeserialize;
use sov_bank::GasTokenConfig;
use sov_blob_storage::{PreferredBlobData, DEFERRED_SLOTS_COUNT, UNREGISTERED_BLOBS_PER_SLOT};
use sov_chain_state::ChainStateConfig;
use sov_kernels::basic::{BasicKernel, BasicKernelGenesisConfig};
use sov_kernels::soft_confirmations::{
    SoftConfirmationsKernel, SoftConfirmationsKernelGenesisConfig,
};
use sov_mock_da::{MockAddress, MockBlob, MockBlock, MockBlockHeader, MockDaSpec};
use sov_modules_api::da::Time;
use sov_modules_api::runtime::capabilities::{BlobSelector, Kernel, KernelSlotHooks};
use sov_modules_api::{
    Address, BlobData, BlobDataWithId, BlobReaderTrait, Context, DaSpec, DispatchCall,
    KernelWorkingSet, MessageCodec, Module, RawTx, Spec, StateCheckpoint,
};
use sov_prover_storage_manager::SimpleStorageManager;
use sov_sequencer_registry::SequencerConfig;
use sov_state::{ProverStorage, Storage};
use sov_test_utils::{
    new_test_blob_from_batch, TestStorageSpec as StorageSpec, TEST_DEFAULT_USER_STAKE,
};
use tracing::{debug, info};

type S = sov_test_utils::TestSpec;
type Da = MockDaSpec;

const PREFERRED_SEQUENCER_DA: MockAddress = MockAddress::new([10u8; 32]);
const PREFERRED_SEQUENCER_ROLLUP: <S as Spec>::Address =
    Address::new(*b"preferred_______________________");
const REGULAR_SEQUENCER_DA: MockAddress = MockAddress::new([30u8; 32]);
const REGULAR_SEQUENCER_ROLLUP: <S as Spec>::Address =
    Address::new(*b"regular_________________________");
const REGULAR_REWARD_ROLLUP: <S as Spec>::Address =
    Address::new(*b"regular_reward__________________");

fn get_bank_config(
    preferred_sequencer: <S as Spec>::Address,
    regular_sequencer: <S as Spec>::Address,
) -> sov_bank::BankConfig<S> {
    let token_name = "InitialToken".to_owned();
    let gas_token_config = GasTokenConfig {
        token_name,
        address_and_balances: vec![
            (preferred_sequencer, TEST_DEFAULT_USER_STAKE * 3),
            (regular_sequencer, TEST_DEFAULT_USER_STAKE * 3),
        ],
        authorized_minters: vec![],
    };

    sov_bank::BankConfig {
        gas_token_config,
        tokens: vec![],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequencerInfo {
    Preferred {
        slots_to_advance: u64,
        sequence_number: u64,
    },
    Regular,
}

fn make_blobs(
    blob_num: &mut u8,
    slot: u64,
    senders_are_preferred: impl Iterator<Item = SequencerInfo>,
) -> Vec<BlobWithAppearance<MockBlob>> {
    let blobs: Vec<_> = senders_are_preferred
        .enumerate()
        .map(|(offset, sequencer_info)| {
            let blob = match sequencer_info {
                SequencerInfo::Preferred {
                    slots_to_advance,
                    sequence_number,
                } => {
                    let txs = vec![RawTx {
                        data: vec![*blob_num],
                    }];

                    MockBlob::new(
                        borsh::to_vec(&PreferredBlobData {
                            data: BlobData::new_batch(txs),
                            sequence_number,
                            virtual_slots_to_advance: slots_to_advance as u8,
                        })
                        .unwrap(),
                        PREFERRED_SEQUENCER_DA,
                        [*blob_num + offset as u8; 32],
                    )
                }
                SequencerInfo::Regular => make_blob(
                    vec![*blob_num],
                    REGULAR_SEQUENCER_DA,
                    [*blob_num + offset as u8; 32],
                ),
            };

            BlobWithAppearance {
                blob,
                appeared_in_slot: slot,
                sequencer_info,
            }
        })
        .collect();
    *blob_num += blobs.len() as u8;
    blobs
}

fn make_blobs_by_slot(
    sequencer_by_slot: &[Vec<SequencerInfo>],
) -> Vec<Vec<BlobWithAppearance<MockBlob>>> {
    let mut blob_num = 0;
    sequencer_by_slot
        .iter()
        .enumerate()
        .map(|(index, senders)| {
            // The first blobs arrive one block after geneseis
            let slot_num = index + 1;
            make_blobs(&mut blob_num, slot_num as u64, senders.iter().cloned())
        })
        .collect()
}

fn make_blob(tx_data: Vec<u8>, sender: MockAddress, id: [u8; 32]) -> MockBlob {
    MockBlob::new(
        borsh::to_vec(&BlobData::new_batch(vec![RawTx { data: tx_data }])).unwrap(),
        sender,
        id,
    )
}

pub struct SlotTestInfo {
    pub slot_number: u64,
    pub expected_virtual_slot: u64,
    /// The expected number of blobs to process, if known
    pub expected_blobs_to_process: Option<usize>,
}

// Tests of the "preferred sequencer" logic tend to have the same structure, which is encoded in this helper:
// 1. Initialize the rollup
// 2. Calculate the expected order of blobs to be processed
// 3. In a loop...
//   Check that the virtual slot number is as expected
//   Assert that blobs are pulled out of the queue in the expected order
// 4. Assert that all blobs have been processed
fn do_deferred_blob_test(
    blobs_by_slot: Vec<Vec<BlobWithAppearance<MockBlob>>>,
    test_info: Vec<SlotTestInfo>,
    first_sequence_number: u64,
) {
    let num_slots = blobs_by_slot.len();
    // Initialize the rollup
    let (current_storage, _runtime, genesis_root) = TestRuntime::pre_initialized(true);

    // Define the kernel
    let mut state_checkpoint = StateCheckpoint::new(current_storage.clone());
    let mut kernel_working_set = KernelWorkingSet::uninitialized(&mut state_checkpoint);
    let test_kernel = SoftConfirmationsKernel::<S, Da>::default();
    test_kernel
        .genesis(
            &SoftConfirmationsKernelGenesisConfig {
                chain_state: ChainStateConfig {
                    current_time: Default::default(),
                    genesis_da_height: 0,
                    inner_code_commitment: Default::default(),
                    outer_code_commitment: Default::default(),
                },
            },
            &mut kernel_working_set,
        )
        .unwrap();

    // Compute the expected order of batches to be executed
    let mut ordered_batches = {
        let mut next_sequence_number = first_sequence_number;
        let mut preferred_batches = HashMap::new();
        let mut batches = Vec::new();
        let mut current_virtual_slot = 0;
        let mut next_virtual_slot = 1;
        for real_slot_num in 0..(blobs_by_slot.len() + DEFERRED_SLOTS_COUNT as usize) {
            let empty_vec = vec![]; // Use a let binding to avoid dropping temporary value
            let slot: &Vec<BlobWithAppearance<MockBlob>> =
                blobs_by_slot.get(real_slot_num).unwrap_or(&empty_vec);
            for blob in slot.iter() {
                if let SequencerInfo::Preferred {
                    sequence_number, ..
                } = blob.sequencer_info
                {
                    preferred_batches.insert(sequence_number, blob.clone());
                }
            }
            if let Some(next_preferred) = preferred_batches.get(&next_sequence_number) {
                batches.push(vec![next_preferred.clone()]);
                next_sequence_number += 1;
                if let SequencerInfo::Preferred {
                    slots_to_advance, ..
                } = next_preferred.sequencer_info
                {
                    next_virtual_slot += std::cmp::max(slots_to_advance, 1);
                } else {
                    panic!("Expected preferred sequencer blob")
                }
                if next_virtual_slot > real_slot_num as u64 + 1 {
                    next_virtual_slot = real_slot_num as u64 + 1;
                }
            } else {
                if next_virtual_slot + DEFERRED_SLOTS_COUNT <= real_slot_num as u64 {
                    next_virtual_slot += 1;
                }
                batches.push(vec![]);
            }

            for slot in blobs_by_slot
                .get(current_virtual_slot as usize..next_virtual_slot as usize)
                .unwrap_or_default()
            {
                for blob in slot {
                    if let SequencerInfo::Regular = blob.sequencer_info {
                        batches.last_mut().unwrap().push(blob.clone());
                    }
                }
            }
            current_virtual_slot = next_virtual_slot;
        }
        debug!(?batches, "Computed the expected batches");
        batches.into_iter()
    };

    let mut slots_iterator = blobs_by_slot
        .into_iter()
        .map(|blobs| blobs.into_iter().map(|b| b.blob).collect())
        .chain(std::iter::repeat(Vec::new()));

    let mut test_info = test_info.into_iter().peekable();

    // Loop enough times that all provided slots are processed and all deferred blobs expire
    for slot_number in 1..=num_slots as u64 + DEFERRED_SLOTS_COUNT {
        // Run the blob selector module
        let slot_number_u8 = slot_number as u8;
        let mut slot_data = MockBlock {
            header: MockBlockHeader {
                prev_hash: [slot_number_u8; 32].into(),
                hash: [slot_number_u8 + 1; 32].into(),
                height: slot_number,
                time: Time::now(),
            },
            validity_cond: Default::default(),
            batch_blobs: slots_iterator.next().unwrap(),
            proof_blobs: Default::default(),
        };

        test_kernel.begin_slot_hook(
            &slot_data.header,
            &slot_data.validity_cond,
            &genesis_root, // For this test, we don't actually execute blocks - so keep reusing the genesis root hash as a placeholder
            &mut state_checkpoint,
        );

        kernel_working_set = KernelWorkingSet::from_kernel(&test_kernel, &mut state_checkpoint);

        let batches_to_execute = test_kernel
            .get_blobs_for_this_slot(&mut slot_data.batch_blobs, &mut kernel_working_set)
            .unwrap();

        assert_eq!(kernel_working_set.current_slot(), slot_number);

        // Run any extra logic provided by the test for this slot
        if let Some(next_slot_info) = test_info.peek() {
            if next_slot_info.slot_number == slot_number {
                assert_eq!(
                    kernel_working_set.virtual_slot(),
                    next_slot_info.expected_virtual_slot
                );
                let next_slot_info = test_info.next().unwrap();
                // If applicable, assert that the expected number of blobs was processed
                if let Some(expected) = next_slot_info.expected_blobs_to_process {
                    info!(
                        slot_number = slot_number,
                        batches = ?batches_to_execute,
                        "selected_batches for slot"
                    );
                    assert_eq!(expected, batches_to_execute.len());
                }
            }
        }

        let batches_for_this_slot = ordered_batches.next().unwrap_or_default();
        debug!(
            "Expected batches for slot {}, {:?}",
            slot_number, batches_for_this_slot
        );
        let mut batches_for_this_slot = batches_for_this_slot.into_iter();
        // Check that the computed list of blobs is the one we expected
        for batch in batches_to_execute {
            let expected: BlobWithAppearance<MockBlob> = batches_for_this_slot.next().unwrap();
            let is_from_preferred = batch.1 == PREFERRED_SEQUENCER_DA;
            assert!(slot_number <= expected.must_be_processed_by());
            assert_blob_matches_batch(
                expected.blob,
                batch,
                &format!("Slot {}", slot_number),
                is_from_preferred,
            );
        }
        assert!(batches_for_this_slot.next().is_none());
    }
    // Ensure that all blobs have been processed
    assert!(ordered_batches.next().is_none());
}

#[test]
fn test_preferred_sequencer_flow() {
    let is_from_preferred_by_slot = [
        vec![
            SequencerInfo::Regular,
            SequencerInfo::Regular,
            SequencerInfo::Preferred {
                slots_to_advance: 1,
                sequence_number: 0,
            },
        ],
        vec![SequencerInfo::Regular, SequencerInfo::Regular],
        vec![
            SequencerInfo::Regular,
            SequencerInfo::Preferred {
                slots_to_advance: 2,
                sequence_number: 1,
            },
            SequencerInfo::Regular,
        ],
        vec![],
        vec![SequencerInfo::Preferred {
            slots_to_advance: 1,
            sequence_number: 2,
        }],
    ];
    let blobs_by_slot: Vec<_> = make_blobs_by_slot(&is_from_preferred_by_slot);
    do_deferred_blob_test(
        blobs_by_slot,
        vec![
            SlotTestInfo {
                slot_number: 1,
                expected_virtual_slot: 1,
                expected_blobs_to_process: Some(3), // In the first slot there's a preferred batch, so we process everything
            },
            SlotTestInfo {
                slot_number: 2,
                expected_virtual_slot: 2,
                expected_blobs_to_process: Some(0), // No preferred batch, we process nothing
            },
            SlotTestInfo {
                slot_number: 3,
                expected_virtual_slot: 2,
                expected_blobs_to_process: Some(5), // Process both deffered blobs from slot 2 and the three from slot 3
            },
            SlotTestInfo {
                slot_number: 4,
                expected_virtual_slot: 4,
                expected_blobs_to_process: Some(0),
            },
            SlotTestInfo {
                slot_number: 5,
                expected_virtual_slot: 4,
                expected_blobs_to_process: Some(1),
            },
        ],
        0,
    );
}

#[test]
fn test_virtual_slot_stays_in_range() {
    let is_from_preferred_by_slot = [
        vec![SequencerInfo::Preferred {
            slots_to_advance: 8,
            sequence_number: 0,
        }],
        vec![SequencerInfo::Regular, SequencerInfo::Regular],
        vec![SequencerInfo::Regular, SequencerInfo::Regular],
        vec![SequencerInfo::Regular, SequencerInfo::Regular],
    ];
    let blobs_by_slot: Vec<_> = make_blobs_by_slot(&is_from_preferred_by_slot);
    do_deferred_blob_test(
        blobs_by_slot,
        vec![
            SlotTestInfo {
                slot_number: 1,
                expected_virtual_slot: 1,
                expected_blobs_to_process: Some(1), // In the first slot there's a preferred batch, so we process everything
            },
            SlotTestInfo {
                slot_number: 2, //
                expected_virtual_slot: 2,
                expected_blobs_to_process: Some(0), // No preferred batch, we process nothing
            },
            SlotTestInfo {
                slot_number: DEFERRED_SLOTS_COUNT + 2,
                expected_virtual_slot: 2,
                expected_blobs_to_process: Some(2),
            },
            SlotTestInfo {
                slot_number: DEFERRED_SLOTS_COUNT + 3,
                expected_virtual_slot: 3,
                expected_blobs_to_process: Some(2),
            },
            SlotTestInfo {
                slot_number: DEFERRED_SLOTS_COUNT + 4,
                expected_virtual_slot: 4,
                expected_blobs_to_process: Some(2),
            },
        ],
        0,
    );
}

#[test]
fn test_recovery_mode() -> Result<(), Infallible> {
    // Initialize the rollup
    let (current_storage, runtime, genesis_root) = TestRuntime::pre_initialized(true);

    // Define the kernel
    let mut state_checkpoint = StateCheckpoint::new(current_storage.clone());
    let mut kernel_working_set = KernelWorkingSet::uninitialized(&mut state_checkpoint);
    let test_kernel = SoftConfirmationsKernel::<S, Da>::default();
    test_kernel
        .genesis(
            &SoftConfirmationsKernelGenesisConfig {
                chain_state: ChainStateConfig {
                    current_time: Default::default(),
                    genesis_da_height: 0,
                    inner_code_commitment: Default::default(),
                    outer_code_commitment: Default::default(),
                },
            },
            &mut kernel_working_set,
        )
        .unwrap();

    // Populate the rollup with deferred blobs
    for slot_number in 1..=DEFERRED_SLOTS_COUNT {
        let slot_number_u8 = slot_number as u8;
        let mut slot_data = MockBlock {
            header: MockBlockHeader {
                prev_hash: [slot_number_u8; 32].into(),
                hash: [slot_number_u8 + 1; 32].into(),
                height: slot_number,
                time: Time::now(),
            },
            validity_cond: Default::default(),
            batch_blobs: vec![
                make_blob(
                    vec![slot_number_u8],
                    REGULAR_SEQUENCER_DA,
                    [slot_number_u8 + 1; 32],
                ),
                make_blob(
                    vec![slot_number_u8 + 128],
                    REGULAR_SEQUENCER_DA,
                    [slot_number_u8 + 128; 32],
                ),
            ],
            proof_blobs: Default::default(),
        };
        test_kernel.begin_slot_hook(
            &slot_data.header,
            &slot_data.validity_cond,
            &genesis_root, // For this test, we don't actually execute blocks - so keep reusing the genesis root hash as a placeholder
            &mut state_checkpoint,
        );
        kernel_working_set = KernelWorkingSet::from_kernel(&test_kernel, &mut state_checkpoint);
        let blobs_to_execute = test_kernel
            .get_blobs_for_this_slot(&mut slot_data.batch_blobs, &mut kernel_working_set)
            .unwrap();
        assert_eq!(kernel_working_set.virtual_slot(), 1);
        assert_eq!(blobs_to_execute.len(), 0);
    }
    // Slash the preferred sequencer and run one block to enter recovery mode
    {
        runtime
            .sequencer_registry
            .slash_sequencer(&PREFERRED_SEQUENCER_DA, &mut state_checkpoint);
    }

    // Ensure that the virtual slot advances two-at a time until it catches up
    for slot_number in DEFERRED_SLOTS_COUNT + 2..DEFERRED_SLOTS_COUNT * 3 {
        let slot_number_u8 = slot_number as u8;
        let mut slot_data = MockBlock {
            header: MockBlockHeader {
                prev_hash: [slot_number_u8; 32].into(),
                hash: [slot_number_u8 + 1; 32].into(),
                height: slot_number,
                time: Time::now(),
            },
            validity_cond: Default::default(),
            batch_blobs: vec![],
            proof_blobs: Default::default(),
        };
        test_kernel.begin_slot_hook(
            &slot_data.header,
            &slot_data.validity_cond,
            &genesis_root, // For this test, we don't actually execute blocks - so keep reusing the genesis root hash as a placeholder
            &mut state_checkpoint,
        );
        kernel_working_set = KernelWorkingSet::from_kernel(&test_kernel, &mut state_checkpoint);
        let blobs_to_execute = test_kernel
            .get_blobs_for_this_slot(&mut slot_data.batch_blobs, &mut kernel_working_set)
            .unwrap();
        let next_height = test_kernel
            .get_chain_state()
            .next_visible_slot_number(&mut kernel_working_set)?;
        if next_height <= DEFERRED_SLOTS_COUNT + 1 {
            assert_eq!(blobs_to_execute.len(), 4);
        } else if next_height == DEFERRED_SLOTS_COUNT + 2 {
            assert_eq!(blobs_to_execute.len(), 2);
        } else {
            assert_eq!(blobs_to_execute.len(), 0);
        }

        match kernel_working_set.virtual_slot().cmp(&slot_number) {
            std::cmp::Ordering::Less => {
                assert_eq!(next_height - kernel_working_set.virtual_slot(), 2);
            }
            std::cmp::Ordering::Equal => {
                assert!(next_height - kernel_working_set.virtual_slot() <= 2);
            }
            std::cmp::Ordering::Greater => {
                panic!("Virtual slot must not advance beyond real slot!")
            }
        }
    }

    Ok(())
}

#[test]
fn test_blobs_from_non_registered_sequencers_are_limited_to_set_amount() {
    let (current_storage, _runtime, genesis_root) = TestRuntime::pre_initialized(true);
    let mut state_checkpoint = StateCheckpoint::new(current_storage.clone());

    // Define the kernel
    let mut kernel_working_set = KernelWorkingSet::uninitialized(&mut state_checkpoint);
    let test_kernel = BasicKernel::<S, Da>::default();
    test_kernel
        .genesis(
            &BasicKernelGenesisConfig {
                chain_state: ChainStateConfig {
                    current_time: Default::default(),
                    genesis_da_height: 0,
                    inner_code_commitment: Default::default(),
                    outer_code_commitment: Default::default(),
                },
            },
            &mut kernel_working_set,
        )
        .unwrap();

    let unregistered_sequencer = MockAddress::from([7; 32]);
    let mut blobs = vec![
        make_blob(vec![1], REGULAR_SEQUENCER_DA, [1u8; 32]),
        make_blob(vec![3, 3, 3], PREFERRED_SEQUENCER_DA, [3u8; 32]),
    ];
    let excessive_unregistered_blobs = 3;

    for _ in 0..UNREGISTERED_BLOBS_PER_SLOT + excessive_unregistered_blobs {
        blobs.push(make_blob(vec![2, 2], unregistered_sequencer, [2u8; 32]));
    }

    assert_eq!(
        blobs.len() as u64,
        2 + UNREGISTERED_BLOBS_PER_SLOT + excessive_unregistered_blobs
    );

    let mut unregistered_blobs_processed = 0;
    let mut registered_blobs_processed = 0;

    for slot_number in 0..DEFERRED_SLOTS_COUNT + 1 {
        let mut slot_data = MockBlock {
            header: MockBlockHeader::from_height(slot_number),
            validity_cond: Default::default(),
            batch_blobs: if slot_number == 0 {
                blobs.clone()
            } else {
                vec![]
            },
            proof_blobs: Default::default(),
        };
        test_kernel.begin_slot_hook(
            &slot_data.header,
            &slot_data.validity_cond,
            &genesis_root, // For this test, we don't actually execute blocks - so keep reusing the genesis root hash as a placeholder
            &mut state_checkpoint,
        );

        kernel_working_set = KernelWorkingSet::from_kernel(&test_kernel, &mut state_checkpoint);
        let blobs_to_execute = test_kernel
            .get_blobs_for_this_slot(&mut slot_data.batch_blobs, &mut kernel_working_set)
            .unwrap();

        for batch in blobs_to_execute {
            if batch.1 == unregistered_sequencer {
                unregistered_blobs_processed += 1;
            } else {
                registered_blobs_processed += 1;
            }
        }
    }
    assert_eq!(registered_blobs_processed, 2);
    // The `excessive_unregistered_blobs` amount of blobs weren't processed we were constrained to
    // `UNREGISTERED_BLOBS_PER_SLOT`
    assert_eq!(unregistered_blobs_processed, UNREGISTERED_BLOBS_PER_SLOT);
}

#[test]
fn test_based_sequencing() -> Result<(), Infallible> {
    let (current_storage, _runtime, genesis_root) = TestRuntime::pre_initialized(false);
    let mut state_checkpoint = StateCheckpoint::new(current_storage.clone());

    // Define the kernel
    let mut kernel_working_set = KernelWorkingSet::uninitialized(&mut state_checkpoint);
    let test_kernel = BasicKernel::<S, Da>::default();
    test_kernel
        .genesis(
            &BasicKernelGenesisConfig {
                chain_state: ChainStateConfig {
                    current_time: Default::default(),
                    genesis_da_height: 0,
                    inner_code_commitment: Default::default(),
                    outer_code_commitment: Default::default(),
                },
            },
            &mut kernel_working_set,
        )
        .unwrap();

    assert_eq!(
        test_kernel
            .chain_state()
            .next_visible_slot_number(&mut kernel_working_set)?,
        1
    );
    assert_eq!(
        test_kernel
            .chain_state()
            .true_slot_number(&mut kernel_working_set)?,
        0
    );

    let blob_1 = make_blob(vec![1], REGULAR_SEQUENCER_DA, [1u8; 32]);
    let blob_2 = make_blob(vec![2, 2], REGULAR_SEQUENCER_DA, [2u8; 32]);
    let blob_3 = make_blob(vec![3, 3, 3], PREFERRED_SEQUENCER_DA, [3u8; 32]);

    let slot_1_blobs = vec![blob_1.clone(), blob_2.clone(), blob_3.clone()];

    let mut slot_1_data = MockBlock {
        header: MockBlockHeader {
            prev_hash: [0; 32].into(),
            hash: [1; 32].into(),
            height: 1,
            time: Time::now(),
        },
        validity_cond: Default::default(),
        batch_blobs: slot_1_blobs,
        proof_blobs: Default::default(),
    };
    test_kernel.begin_slot_hook(
        &slot_1_data.header,
        &slot_1_data.validity_cond,
        &genesis_root, // For this test, we don't actually execute blocks - so keep reusing the genesis root hash as a placeholder
        &mut state_checkpoint,
    );
    kernel_working_set = KernelWorkingSet::from_kernel(&test_kernel, &mut state_checkpoint);
    let mut execute_in_slot_1 = test_kernel
        .get_blobs_for_this_slot(&mut slot_1_data.batch_blobs, &mut kernel_working_set)
        .unwrap();
    assert_eq!(3, execute_in_slot_1.len());
    assert_blob_matches_batch(blob_1, execute_in_slot_1.remove(0), "slot 1", false);
    assert_blob_matches_batch(blob_2, execute_in_slot_1.remove(0), "slot 1", false);
    assert_blob_matches_batch(blob_3, execute_in_slot_1.remove(0), "slot 1", false);
    assert_eq!(
        test_kernel
            .chain_state()
            .true_slot_number(&mut kernel_working_set)?,
        1
    );
    assert_eq!(kernel_working_set.virtual_slot(), 1);

    let mut slot_2_data = MockBlock {
        header: MockBlockHeader {
            prev_hash: slot_1_data.header.hash,
            hash: [2; 32].into(),
            height: 2,
            time: Time::now(),
        },
        validity_cond: Default::default(),
        batch_blobs: Vec::new(),
        proof_blobs: Default::default(),
    };
    test_kernel.begin_slot_hook(
        &slot_2_data.header,
        &slot_2_data.validity_cond,
        &genesis_root, // For this test, we don't actually execute blocks - so keep reusing the genesis root hash as a placeholder
        &mut state_checkpoint,
    );
    kernel_working_set = KernelWorkingSet::from_kernel(&test_kernel, &mut state_checkpoint);
    let execute_in_slot_2 = test_kernel
        .get_blobs_for_this_slot(&mut slot_2_data.batch_blobs, &mut kernel_working_set)
        .unwrap();
    assert_eq!(
        test_kernel
            .chain_state()
            .true_slot_number(&mut kernel_working_set)?,
        2
    );
    assert_eq!(kernel_working_set.virtual_slot(), 2);
    assert!(execute_in_slot_2.is_empty());

    Ok(())
}

#[test]
fn test_based_sequencing_unregistered_blobs() -> Result<(), Infallible> {
    let (current_storage, _runtime, genesis_root) = TestRuntime::pre_initialized(false);
    let mut state_checkpoint = StateCheckpoint::new(current_storage.clone());

    // Define the kernel
    let mut kernel_working_set = KernelWorkingSet::uninitialized(&mut state_checkpoint);
    let test_kernel = BasicKernel::<S, Da>::default();
    test_kernel
        .genesis(
            &BasicKernelGenesisConfig {
                chain_state: ChainStateConfig {
                    current_time: Default::default(),
                    genesis_da_height: 0,
                    inner_code_commitment: Default::default(),
                    outer_code_commitment: Default::default(),
                },
            },
            &mut kernel_working_set,
        )
        .unwrap();

    assert_eq!(
        test_kernel
            .chain_state()
            .next_visible_slot_number(&mut kernel_working_set)?,
        1
    );
    assert_eq!(
        test_kernel
            .chain_state()
            .true_slot_number(&mut kernel_working_set)?,
        0
    );

    let unregistered_sequencer = MockAddress::from([7; 32]);
    let blob_1 = make_blob(vec![1], REGULAR_SEQUENCER_DA, [1u8; 32]);
    let blob_2 = make_blob(vec![2, 2], REGULAR_SEQUENCER_DA, [2u8; 32]);
    let blob_3 = make_blob(vec![3, 3, 3], PREFERRED_SEQUENCER_DA, [3u8; 32]);
    let unregistered_blob = make_blob(vec![2, 2], unregistered_sequencer, [2u8; 32]);
    let mut blobs = vec![blob_1.clone(), blob_2.clone(), blob_3.clone()];
    let excessive_unregistered_blobs = 3;

    for _ in 0..UNREGISTERED_BLOBS_PER_SLOT + excessive_unregistered_blobs {
        blobs.push(unregistered_blob.clone());
    }

    assert_eq!(
        blobs.len() as u64,
        3 + UNREGISTERED_BLOBS_PER_SLOT + excessive_unregistered_blobs
    );

    let mut slot_1_data = MockBlock {
        header: MockBlockHeader::from_height(1),
        validity_cond: Default::default(),
        batch_blobs: blobs,
        proof_blobs: Default::default(),
    };
    test_kernel.begin_slot_hook(
        &slot_1_data.header,
        &slot_1_data.validity_cond,
        &genesis_root, // For this test, we don't actually execute blocks - so keep reusing the genesis root hash as a placeholder
        &mut state_checkpoint,
    );
    kernel_working_set = KernelWorkingSet::from_kernel(&test_kernel, &mut state_checkpoint);
    let mut execute_in_slot_1 = test_kernel
        .get_blobs_for_this_slot(&mut slot_1_data.batch_blobs, &mut kernel_working_set)
        .unwrap();

    // assert no extra unregistered blobs were added
    assert_eq!(
        3 + UNREGISTERED_BLOBS_PER_SLOT,
        execute_in_slot_1.len() as u64
    );

    assert_blob_matches_batch(blob_1, execute_in_slot_1.remove(0), "slot 1", false);
    assert_blob_matches_batch(blob_2, execute_in_slot_1.remove(0), "slot 1", false);
    assert_blob_matches_batch(blob_3, execute_in_slot_1.remove(0), "slot 1", false);

    for _ in 0..UNREGISTERED_BLOBS_PER_SLOT {
        assert_blob_matches_batch(
            unregistered_blob.clone(),
            execute_in_slot_1.remove(0),
            "slot 1",
            false,
        );
    }

    assert_eq!(
        test_kernel
            .chain_state()
            .true_slot_number(&mut kernel_working_set)?,
        1
    );
    assert_eq!(kernel_working_set.virtual_slot(), 1);

    Ok(())
}

/// Check hashes and data of two blobs.
fn assert_blob_matches_batch<B: BlobReaderTrait>(
    mut expected: B,
    actual: (BlobDataWithId, B::Address),
    slot_hint: &str,
    is_preferred: bool,
) {
    // Reconstruct the original blob from the batch and its sender
    let actual_id = actual.0.id;
    if is_preferred {
        assert_eq!(expected.hash(), actual.0.id);
        let expected = PreferredBlobData::try_from_slice(expected.full_data()).unwrap();
        assert_eq!(expected.data, actual.0.data);
    } else {
        let batch = match actual.0.data {
            BlobData::Batch(batch) => batch,
            BlobData::Proof(_) => panic!("Expected a batch, but got a proof"),
        };

        let mut actual_inner = new_test_blob_from_batch(batch, actual.1.as_ref(), actual_id);
        assert_eq!(
            expected.hash(),
            actual_inner.hash(),
            "incorrect hashes in {}",
            slot_hint
        );

        assert_eq!(
            actual_inner.full_data(),
            expected.full_data(),
            "incorrect data read in {}",
            slot_hint
        );
    }
}

/// A utility struct to allow easy expected ordering of blobs
#[derive(PartialEq, Clone, Debug)]
struct BlobWithAppearance<B> {
    pub blob: B,
    appeared_in_slot: u64,
    sequencer_info: SequencerInfo,
}

impl<B> BlobWithAppearance<B> {
    pub fn must_be_processed_by(&self) -> u64 {
        match self.sequencer_info {
            SequencerInfo::Preferred {
                slots_to_advance: _slots_to_advance,
                sequence_number,
            } => self.appeared_in_slot + sequence_number,
            SequencerInfo::Regular => self.appeared_in_slot + DEFERRED_SLOTS_COUNT,
        }
    }
}

#[derive(Default, sov_modules_api::Genesis, DispatchCall, MessageCodec)]
#[serialization(borsh::BorshDeserialize, borsh::BorshSerialize)]
struct TestRuntime<S: Spec, Da: DaSpec> {
    pub bank: sov_bank::Bank<S>,
    pub sequencer_registry: sov_sequencer_registry::SequencerRegistry<S, Da>,
}

impl TestRuntime<S, MockDaSpec> {
    pub fn pre_initialized(
        with_preferred_sequencer: bool,
    ) -> (
        ProverStorage<StorageSpec>,
        Self,
        <ProverStorage<StorageSpec> as Storage>::Root,
    ) {
        use sov_modules_api::Genesis;
        let tmpdir = tempfile::tempdir().unwrap();
        let mut storage_manager = SimpleStorageManager::new(tmpdir.path());
        let storage = storage_manager.create_storage();

        let genesis_config = Self::build_genesis_config(with_preferred_sequencer);
        let runtime: Self = Default::default();

        let state = StateCheckpoint::<S>::new(storage.clone());
        let mut genesis_state =
            state.to_genesis_state_accessor::<TestRuntime<S, MockDaSpec>>(&genesis_config);
        runtime
            .genesis(&genesis_config, &mut genesis_state)
            .unwrap();
        let mut state = genesis_state.checkpoint().to_working_set_unmetered();

        // In addition to "genesis", register one non-preferred sequencer
        let register_message = sov_sequencer_registry::CallMessage::Register {
            da_address: REGULAR_SEQUENCER_DA.as_ref().to_vec(),
            amount: TEST_DEFAULT_USER_STAKE,
        };
        runtime
            .sequencer_registry
            .call(
                register_message,
                &Context::<S>::new(
                    REGULAR_SEQUENCER_ROLLUP,
                    Default::default(),
                    REGULAR_REWARD_ROLLUP,
                    1,
                ),
                &mut state,
            )
            .unwrap();

        let (reads_writes, _, witness) = state.checkpoint().0.freeze();
        let (genesis_root, change_set) = storage
            .validate_and_materialize(reads_writes, &witness)
            .unwrap();

        storage_manager.commit(change_set);

        // let root = storage.validate_and_commit()
        (storage_manager.create_storage(), runtime, genesis_root)
    }

    fn build_genesis_config(with_preferred_sequencer: bool) -> GenesisConfig<S, MockDaSpec> {
        let bank_config = get_bank_config(PREFERRED_SEQUENCER_ROLLUP, REGULAR_SEQUENCER_ROLLUP);

        let sequencer_registry_config = SequencerConfig {
            seq_rollup_address: PREFERRED_SEQUENCER_ROLLUP,
            seq_da_address: PREFERRED_SEQUENCER_DA,
            minimum_bond: TEST_DEFAULT_USER_STAKE,
            is_preferred_sequencer: with_preferred_sequencer,
        };

        GenesisConfig {
            bank: bank_config,
            sequencer_registry: sequencer_registry_config,
        }
    }
}
