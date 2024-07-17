//! All code related to handling storage manager anb ledger.
use std::collections::VecDeque;

use sov_db::ledger_db::{LedgerDb, SlotCommit};
use sov_db::schema::{CacheDb, SchemaBatch};
use sov_rollup_interface::da::{BlockHeaderTrait, DaSpec};
use sov_rollup_interface::services::da::{DaService, SlotData};
use sov_rollup_interface::stf::TxReceiptContents;
use sov_rollup_interface::storage::HierarchicalStorageManager;
use sov_rollup_interface::zk::aggregated_proof::AggregatedProof;
use sov_rollup_interface::zk::StateTransitionWitness;
use tokio::sync::watch;

use crate::StateTransitionInfo;

/// Point where rollup execution can be resumed after DA fork happened.
struct ForkPoint<Da: DaService, StateRoot> {
    /// The next block in a new fork, following the last seen transition by the rollup.
    block: Da::FilteredBlock,
    /// Last observed state root before the fork.
    pre_state_root: StateRoot,
}

/// StateManager controls storage lifecycle for [`StateTransitionFunction`],
/// [`LedgerDb`] and RPC endpoints in case of DA-reorgs.
/// It needs [`DaService`] so it can backtrack to the last seen transition in new fork.
pub struct StateManager<StateRoot, Witness, Sm, Da>
where
    Da: DaService,
    Sm: HierarchicalStorageManager<Da::Spec>,
{
    storage_manager: Sm,
    ledger_db: LedgerDb,
    // `state_root` is tracked so [`StateTransitionWitness`] can have proper `prev_state_root`.
    // Probably it can be saved in variable before "apply_slot" is called,
    // But then the runner needs to know about it and carry it over.
    state_root: StateRoot,
    seen_state_transitions: VecDeque<StateTransitionInfo<StateRoot, Witness, Da::Spec>>,
    rpc_storage_sender: watch::Sender<Sm::StfState>,
}

impl<StateRoot, Witness, Sm, Da> StateManager<StateRoot, Witness, Sm, Da>
where
    Da: DaService<Error = anyhow::Error> + Clone,
    StateRoot: Clone + AsRef<[u8]>,
    Sm: HierarchicalStorageManager<Da::Spec, LedgerChangeSet = SchemaBatch, LedgerState = CacheDb>,
    Sm::StfState: Clone,
{
    pub(crate) fn new(
        storage_manager: Sm,
        ledger_db: LedgerDb,
        initial_state_root: StateRoot,
        rpc_storage_sender: watch::Sender<Sm::StfState>,
    ) -> Self {
        Self {
            storage_manager,
            ledger_db,
            state_root: initial_state_root,
            seen_state_transitions: Default::default(),
            rpc_storage_sender,
        }
    }

    /// Returns an [`HierarchicalStorageManager::StfState`] and a [`DaService::FilteredBlock`] that can be used to continue execution.
    /// If a caller relies on some data from `filtered_block`,
    /// it should be updated after call of this method.
    /// If a given block continues in the current fork, it is simply returned to the caller.
    /// If reorg happened, it will return block following the last seen transition.
    pub(crate) async fn prepare_storage(
        &mut self,
        mut filtered_block: Da::FilteredBlock,
        da_service: &Da,
    ) -> anyhow::Result<(Sm::StfState, Da::FilteredBlock)> {
        let reorg_happened = if let Some(ForkPoint {
            block: new_block,
            pre_state_root,
        }) = self
            .has_reorg_happened(filtered_block.header(), da_service)
            .await?
        {
            filtered_block = new_block;
            self.state_root = pre_state_root;
            tracing::info!(
                header = %filtered_block.header().display(),
                "Resuming execution at fork point's height"
            );
            true
        } else {
            false
        };

        let (stf_pre_state, ledger_state) = self
            .storage_manager
            .create_state_for(filtered_block.header())?;
        if reorg_happened {
            tracing::trace!(
                "Reorg has happened, updating RPC and Ledger storage before returning Stf state"
            );
            // In case if reorg happened, we want to keep ledger and RPC storages in sync.
            // Otherwise, the RPC storage and LedgerDb have been updated in [`Self::update_rpc_and_ledger_storage`]
            self.rpc_storage_sender.send_replace(stf_pre_state.clone());
            self.ledger_db.replace_db(ledger_state)?;
        }

        tracing::trace!(block_header = %filtered_block.header().display(), "Returning STF state for block");
        Ok((stf_pre_state, filtered_block))
    }

    /// Performs all necessary operations on data that has been processed by the rollup.
    /// Returns vector of finalized state transitions, so caller can do anything on top of that.
    /// All necessary data for these finalized transitions have been saved on disk.
    pub(crate) async fn process_stf_changes<
        S: SlotData,
        B: serde::Serialize,
        T: TxReceiptContents,
    >(
        &mut self,
        last_finalized_height: u64,
        stf_changes: Sm::StfChangeSet,
        transition_witness: StateTransitionWitness<StateRoot, Witness, Da::Spec>,
        slot_commit: SlotCommit<S, B, T>,
        aggregated_proofs: Vec<AggregatedProof>,
    ) -> anyhow::Result<Vec<StateTransitionInfo<StateRoot, Witness, Da::Spec>>> {
        let slot_number = self.get_slot_number()?;
        let new_state_root = transition_witness.final_state_root.clone();
        let block_header = transition_witness.da_block_header.clone();
        tracing::debug!(
            slot_number,
            block_header = %block_header.display(),
            current_state_root = hex::encode(self.get_state_root().as_ref()),
            next_state_root = hex::encode(new_state_root.as_ref()),
            "Saving changes after applying slot"
        );
        self.seen_state_transitions.push_back(StateTransitionInfo {
            data: transition_witness,
            slot_number,
        });

        let (finalization_ledger_changes, finalized_transitions) = self
            .process_finalized_state_transitions(last_finalized_height)
            .await?;

        let mut ledger_change_set = self
            .ledger_db
            .materialize_slot(slot_commit, new_state_root.as_ref())?;
        for aggregated_proof in aggregated_proofs {
            let this_height_data = self
                .ledger_db
                .materialize_aggregated_proof(aggregated_proof)?;
            ledger_change_set.merge(this_height_data);
        }
        ledger_change_set.merge(finalization_ledger_changes);

        self.storage_manager
            .save_change_set(&block_header, stf_changes, ledger_change_set)?;

        self.update_rpc_and_ledger_storage(&block_header)?;
        for finalized_transition in &finalized_transitions {
            self.storage_manager
                .finalize(finalized_transition.da_block_header())?;
        }
        self.state_root = new_state_root;
        // RPC storage and Ledger have all data from this iteration,
        // now it is safe to submit notifications.
        self.ledger_db.send_notifications();

        Ok(finalized_transitions)
    }

    /// Checks if passed [`DaSpec::BlockHeader`] is a continuation of seen state transition.
    /// If not, traverses back to the latest seen transition, that belongs to the fork of passed header.
    async fn has_reorg_happened(
        &mut self,
        block_header: &<Da::Spec as DaSpec>::BlockHeader,
        da_service: &Da,
    ) -> anyhow::Result<Option<ForkPoint<Da, StateRoot>>> {
        if let Some(state_transition) = self.seen_state_transitions.back() {
            if state_transition.da_block_header().hash() != block_header.prev_hash() {
                tracing::warn!(
                    current_header = %block_header.display(),
                    prev_seen_header = %state_transition.da_block_header().display(),
                    "Block does not belong in current chain. Chain has forked. Traversing seen headers backwards"
                );
                while let Some(state_transition) = self.seen_state_transitions.pop_back() {
                    let block = da_service
                        .get_block_at(state_transition.da_block_header().height())
                        .await?;
                    tracing::debug!(
                        fetched = %block.header().display(),
                        seen = %state_transition.da_block_header().display(),
                        "Checking seen header vs fetched from DA"
                    );
                    if block.header().prev_hash() == state_transition.da_block_header().prev_hash()
                    {
                        return Ok(Some(ForkPoint {
                            block,
                            pre_state_root: state_transition.initial_state_root().clone(),
                        }));
                    }
                }
                //
                anyhow::bail!("Could not match any seen block with the current chain. Could rollup start from non-finalized block?");
            }
        }
        Ok(None)
    }

    /// Returns all [`StateTransitionInfo`] which are below finalized height
    /// and relevant LedgerDb changes.
    async fn process_finalized_state_transitions(
        &mut self,
        last_finalized_height: u64,
    ) -> anyhow::Result<(
        SchemaBatch,
        Vec<StateTransitionInfo<StateRoot, Witness, Da::Spec>>,
    )> {
        tracing::debug!(
            last_finalized_height,
            seen_transitions = self.seen_state_transitions.len(),
            "Start processing finalized state transitions"
        );
        let mut ledger_change_set = SchemaBatch::new();
        let mut finalized_transitions = Vec::new();
        // Checking all seen blocks, in case if there was delay in getting last finalized header.
        while let Some(earliest_seen_state_transition_info) = self.seen_state_transitions.front() {
            let earliest_header = earliest_seen_state_transition_info.da_block_header();
            tracing::debug!(header = %earliest_header.display(), last_finalized_height, "Checking seen header");
            let height = earliest_header.height();

            if height <= last_finalized_height {
                ledger_change_set = self.ledger_db.materialize_latest_finalize_slot(
                    earliest_seen_state_transition_info.slot_number,
                )?;

                let transition_data = self.seen_state_transitions.pop_front().expect(
                    "There should be seen transition, as observed by previous call to .front()",
                );

                finalized_transitions.push(transition_data);
                continue;
            }

            break;
        }
        tracing::debug!(
            finalized_transitions = finalized_transitions.len(),
            "Completed check for finalized transitions"
        );
        Ok((ledger_change_set, finalized_transitions))
    }

    fn update_rpc_and_ledger_storage(
        &mut self,
        block_header: &<<Da as DaService>::Spec as DaSpec>::BlockHeader,
    ) -> anyhow::Result<()> {
        tracing::debug!(after_block = %block_header.display(), "Updating Ledger and RPC storage");
        let (rpc_storage, ledger_state) = self.storage_manager.create_state_after(block_header)?;

        // `send_replace` is superior to `send` for our use case. It never fails
        // because it doesn't need to notify all receivers, unlike `send`, which
        // we don't need. It will also keep working even if there are no
        // receivers currently alive, which makes it easier to reason about the
        // code.
        self.rpc_storage_sender.send_replace(rpc_storage);
        self.ledger_db.replace_db(ledger_state)?;
        Ok(())
    }

    /// Allows reading current state root.
    pub fn get_state_root(&self) -> &StateRoot {
        &self.state_root
    }

    fn get_slot_number(&self) -> anyhow::Result<u64> {
        Ok(self.ledger_db.get_next_items_numbers()?.slot_number)
    }
}

#[cfg(test)]
mod tests {
    use sov_mock_da::{
        MockAddress, MockBlock, MockBlockHeader, MockDaService, MockDaSpec, MockFee,
        MockValidityCond, PlannedFork,
    };
    use sov_mock_zkvm::MockZkvm;
    use sov_modules_stf_blueprint::TxReceiptContents;
    use sov_prover_storage_manager::ProverStorageManager;
    use sov_rollup_interface::services::da::DaServiceWithRetries;
    use sov_rollup_interface::stf::StateTransitionFunction;
    use sov_rollup_interface::zk::{ZkvmGuest, ZkvmHost};
    use sov_state::{
        ArrayWitness, NativeStorage, ProverChangeSet, ProverStorage, SlotKey, SlotValue,
        StateAccesses, Storage,
    };

    use super::*;
    use crate::mock::MockStf;
    type Da = DaServiceWithRetries<MockDaService>;
    type Vm = MockZkvm;
    type Stf = MockStf<MockValidityCond>;
    type S = sov_state::DefaultStorageSpec<sha2::Sha256>;
    type StateRoot = <Stf as StateTransitionFunction<
        <<Vm as ZkvmHost>::Guest as ZkvmGuest>::Verifier,
        <<Vm as ZkvmHost>::Guest as ZkvmGuest>::Verifier,
        MockDaSpec,
    >>::StateRoot;
    type Witness = <Stf as StateTransitionFunction<
        <<Vm as ZkvmHost>::Guest as ZkvmGuest>::Verifier,
        <<Vm as ZkvmHost>::Guest as ZkvmGuest>::Verifier,
        MockDaSpec,
    >>::Witness;
    type MockSlotCommit = SlotCommit<MockBlock, Witness, TxReceiptContents>;
    type TestStateManager =
        StateManager<StateRoot, Witness, ProverStorageManager<MockDaSpec, S>, Da>;

    const SEQUENCER_ADDRESS: MockAddress = MockAddress::new([0; 32]);

    async fn setup_state_manager(path: &std::path::Path) -> anyhow::Result<TestStateManager> {
        let storage_config = sov_state::config::Config {
            path: path.to_path_buf(),
        };

        let mut storage_manager: ProverStorageManager<MockDaSpec, S> =
            ProverStorageManager::new(storage_config)?;
        let genesis_header = MockBlockHeader::from_height(0);
        let (genesis_storage, ledger_state) = storage_manager.create_state_for(&genesis_header)?;
        let ledger_db = LedgerDb::with_cache_db(ledger_state)?;
        let rpc_storage_sender = watch::Sender::new(genesis_storage.clone());

        let (state_root, change_set) = produce_synthetic_changes(&genesis_storage, 0);

        storage_manager.save_change_set(&genesis_header, change_set, SchemaBatch::new())?;

        Ok(StateManager::new(
            storage_manager,
            ledger_db,
            state_root,
            rpc_storage_sender,
        ))
    }

    fn produce_synthetic_changes(
        prover_storage: &ProverStorage<S>,
        height: u64,
    ) -> (StateRoot, ProverChangeSet) {
        let w = ArrayWitness::default();
        let mut accesses = StateAccesses::default();
        accesses.user.ordered_writes.push((
            SlotKey::from(vec![height as u8]),
            Some(SlotValue::from(vec![height as u8])),
        ));
        let (state_root, state_update) = prover_storage.compute_state_update(accesses, &w).unwrap();
        let change_set = prover_storage.materialize_changes(&state_update);

        (state_root.root_hash().0.to_vec(), change_set)
    }

    async fn process_normal_transition(
        state_manager: &mut TestStateManager,
        filtered_block: MockBlock,
        finalized_height: u64,
        da_service: &Da,
    ) -> anyhow::Result<Vec<StateTransitionInfo<StateRoot, (), MockDaSpec>>> {
        let (prover_storage, returned_block) = state_manager
            .prepare_storage(filtered_block.clone(), da_service)
            .await?;

        assert_eq!(filtered_block, returned_block);

        let (state_root, change_set) =
            produce_synthetic_changes(&prover_storage, filtered_block.header().height());
        let (relevant_blobs, relevant_proofs) = da_service
            .extract_relevant_blobs_with_proof(&filtered_block)
            .await;

        let transition_witness = StateTransitionWitness {
            initial_state_root: state_manager.get_state_root().to_owned(),
            final_state_root: state_root,
            da_block_header: filtered_block.header().clone(),
            relevant_proofs,
            relevant_blobs,
            witness: (),
        };

        let slot_commit: MockSlotCommit = SlotCommit::new(filtered_block);
        let finalized = state_manager
            .process_stf_changes(
                finalized_height,
                change_set,
                transition_witness,
                slot_commit,
                Vec::new(),
            )
            .await?;

        Ok(finalized)
    }

    #[tokio::test]
    async fn state_manager_on_empty_transitions_non_instant_finalization() -> anyhow::Result<()> {
        // Checks that the same block returned when storage requested on new
        let tempdir = tempfile::tempdir()?;
        let mut state_manager = setup_state_manager(tempdir.path()).await?;

        let filtered_block = MockBlock {
            header: MockBlockHeader::from_height(1),
            ..Default::default()
        };
        let da_service = MockDaService::new(SEQUENCER_ADDRESS);
        let da_service = DaServiceWithRetries::new_fast(da_service);

        let finalized =
            process_normal_transition(&mut state_manager, filtered_block, 0, &da_service).await?;
        assert!(finalized.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_instant_finality() -> anyhow::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut state_manager = setup_state_manager(tempdir.path()).await?;
        let da_service = DaServiceWithRetries::new_fast(MockDaService::new(SEQUENCER_ADDRESS));

        let mut state_root = state_manager.get_state_root().clone();
        for height in 1..4 {
            da_service
                .send_transaction(&[height as u8; 10], MockFee::zero())
                .await?;
            let filtered_block = da_service.get_block_at(height).await?;
            let mut finalized = process_normal_transition(
                &mut state_manager,
                filtered_block.clone(),
                height,
                &da_service,
            )
            .await?;
            assert_eq!(1, finalized.len());
            let finalized = finalized.pop().unwrap();
            assert_eq!(height - 1, finalized.slot_number);
            assert_eq!(filtered_block.header, finalized.data.da_block_header);
            assert_eq!(state_root, finalized.data.initial_state_root);
            state_root = finalized.data.final_state_root;
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_reorg_happened_correct_block_returned() -> anyhow::Result<()> {
        // The idea of the test is
        // to check that state manager returns the correct block and storage after reorg.
        let tempdir = tempfile::tempdir()?;
        let mut state_manager = setup_state_manager(tempdir.path()).await?;

        let fork_point = 3;
        let last_block = 5;

        let storage_receiver = state_manager.rpc_storage_sender.subscribe();

        let mut da_service = MockDaService::new(SEQUENCER_ADDRESS).with_finality(5);
        da_service
            .set_planned_fork(PlannedFork::new(
                last_block,
                fork_point,
                vec![vec![11], vec![22], vec![33], vec![44]],
            ))
            .await?;
        let da_service = DaServiceWithRetries::new_fast(da_service);

        let mut state_roots = Vec::with_capacity(last_block as usize);

        for height in 1..=last_block {
            da_service
                .send_transaction(&[height as u8; 10], MockFee::zero())
                .await?;
            let filtered_block = da_service.get_block_at(height).await?;
            if height < last_block {
                process_normal_transition(&mut state_manager, filtered_block, 0, &da_service)
                    .await?;
                let current_state_root = state_manager.get_state_root().clone();
                let received_storage = storage_receiver.borrow().clone();
                let received_storage_root = received_storage.get_root_hash(height)?;
                assert_eq!(
                    current_state_root,
                    received_storage_root.root_hash().0.to_vec()
                );
                state_roots.push(current_state_root);
            } else {
                let (prover_storage, returned_block) = state_manager
                    .prepare_storage(filtered_block.clone(), &da_service)
                    .await?;

                assert_ne!(filtered_block, returned_block);
                assert_eq!(fork_point + 1, returned_block.header().height());

                let expected_state_root = &state_roots[fork_point as usize - 1];
                assert_eq!(expected_state_root, state_manager.get_state_root());

                let returned_storage_root = prover_storage.get_root_hash(fork_point)?;
                let received_storage = storage_receiver.borrow().clone();
                let received_storage_root = received_storage.get_root_hash(fork_point)?;
                assert_eq!(returned_storage_root, received_storage_root);
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_no_seen_block_has_been_tracked() -> anyhow::Result<()> {
        // The idea of the test, is that state manager receives request for storage for a block
        // That is not a part of the current chain.
        // But it cannot back-track to last known block in new chain,
        // because it hasn't seen transition in new chain.
        // We simulate that by removing seen transitions before actual finalization happens.

        let tempdir = tempfile::tempdir()?;
        let mut state_manager = setup_state_manager(tempdir.path()).await?;

        let chain_length = 5;
        let finality = 3;
        let da_service = DaServiceWithRetries::new_fast(
            MockDaService::new(SEQUENCER_ADDRESS).with_finality(finality),
        );

        for height in 1..=chain_length {
            da_service
                .send_transaction(&[height as u8; 10], MockFee::zero())
                .await?;
            let filtered_block = da_service.get_block_at(height).await?;
            let finalized_height = height.saturating_sub(finality as u64);
            let _finalized = process_normal_transition(
                &mut state_manager,
                filtered_block.clone(),
                finalized_height,
                &da_service,
            )
            .await?;
        }

        // Reinitialize new da_service with completely different blocks.
        let da_service = MockDaService::new(SEQUENCER_ADDRESS).with_finality(finality);
        for height in 1..=chain_length {
            da_service
                .send_transaction(&[(height * 10) as u8; 10], MockFee::zero())
                .await?;
        }
        let da_service = DaServiceWithRetries::new_fast(da_service);

        let alien_block = da_service.get_block_at(chain_length).await?;

        let result = state_manager
            .prepare_storage(alien_block, &da_service)
            .await;
        assert!(result.is_err());
        assert_eq!("Could not match any seen block with the current chain. Could rollup start from non-finalized block?", result.unwrap_err().to_string());

        Ok(())
    }

    #[tokio::test]
    async fn test_save_last_finalized_larger_than_seen_transitions() -> anyhow::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut state_manager = setup_state_manager(tempdir.path()).await?;
        let da_service = DaServiceWithRetries::new_fast(MockDaService::new(SEQUENCER_ADDRESS));

        let chain_length = 5;
        // Fill some seen transitions without finalizing.
        for height in 1..chain_length {
            da_service
                .send_transaction(&[height as u8; 10], MockFee::zero())
                .await?;
            let filtered_block = da_service.get_block_at(height).await?;

            let finalized =
                process_normal_transition(&mut state_manager, filtered_block, 0, &da_service)
                    .await?;
            assert_eq!(0, finalized.len());
        }
        da_service
            .send_transaction(&[chain_length as u8; 10], MockFee::zero())
            .await?;
        let filtered_block = da_service.get_block_at(chain_length).await?;

        let finalized =
            process_normal_transition(&mut state_manager, filtered_block, u64::MAX, &da_service)
                .await?;
        // When finalized height is larger than seen transition, it simply means that we will finalize all available
        assert_eq!(chain_length as usize, finalized.len());

        Ok(())
    }
}
