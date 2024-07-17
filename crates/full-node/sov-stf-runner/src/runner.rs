use std::net::SocketAddr;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;

use jsonrpsee::RpcModule;
use sov_db::ledger_db::{LedgerDb, SlotCommit};
use sov_db::schema::{CacheDb, SchemaBatch};
use sov_rollup_interface::da::{BlobReaderTrait, BlockHeaderTrait, DaSpec};
use sov_rollup_interface::services::da::{DaService, SlotData};
use sov_rollup_interface::stf::StateTransitionFunction;
use sov_rollup_interface::storage::HierarchicalStorageManager;
use sov_rollup_interface::zk::{StateTransitionWitness, Zkvm, ZkvmGuest, ZkvmHost};
use tokio::sync::watch;
use tracing::{debug, error, info};

use crate::state_manager::StateManager;
use crate::{ProofManager, ProverService, RunnerConfig, StateTransitionInfo};

type GenesisParams<ST, InnerVm, OuterVm, Da> =
    <ST as StateTransitionFunction<InnerVm, OuterVm, Da>>::GenesisParams;

type Verifier<Host> = <<Host as ZkvmHost>::Guest as ZkvmGuest>::Verifier;

/// Combines `DaService` with `StateTransitionFunction` and "runs" the rollup.
pub struct StateTransitionRunner<Stf, Sm, Da, InnerVm, OuterVm, Ps>
where
    Da: DaService,
    InnerVm: ZkvmHost,
    OuterVm: ZkvmHost,
    Sm: HierarchicalStorageManager<Da::Spec>,
    Stf: StateTransitionFunction<
        Verifier<InnerVm>,
        Verifier<OuterVm>,
        Da::Spec,
        Condition = <Da::Spec as DaSpec>::ValidityCondition,
    >,
    Ps: ProverService,
{
    first_unprocessed_height_at_startup: u64,
    da_polling_interval_ms: u64,
    da_service: Arc<Da>,
    stf: Stf,
    state_manager: StateManager<Stf::StateRoot, Stf::Witness, Sm, Da>,
    listen_address_rpc: SocketAddr,
    listen_address_axum: SocketAddr,
    proof_manager: ProofManager<Ps>,
    sync_state: Arc<DaSyncState>,
}

/// The state necessary to track the sync status of the node
#[derive(Debug, Default)]
pub struct DaSyncState {
    synced_da_height: AtomicU64,
    target_da_height: AtomicU64,
}

/// The status of the current sync
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyncStatus {
    /// The node has caught up to the chain tip
    Synced {
        /// The current height through which we've synced
        synced_da_height: u64,
    },
    /// The node is currently syncing
    Syncing {
        /// The current height through which we've synced
        synced_da_height: u64,
        /// The height to which we're syncing. This reflects the current view of the DA chain tip
        target_da_height: u64,
    },
}

impl SyncStatus {
    /// Returns true if the sync status is `Synced`
    pub fn is_synced(&self) -> bool {
        match self {
            SyncStatus::Synced { .. } => true,
            SyncStatus::Syncing { .. } => false,
        }
    }
}

impl DaSyncState {
    async fn update_target<Da: DaService<Error = anyhow::Error>>(
        &self,
        da_service: &Da,
    ) -> Result<(), anyhow::Error> {
        let target_da_height = da_service.get_head_block_header().await?.height();
        self.target_da_height
            .store(target_da_height, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    fn status(&self) -> SyncStatus {
        let current = self
            .synced_da_height
            .load(std::sync::atomic::Ordering::Acquire);
        let target = self
            .target_da_height
            .load(std::sync::atomic::Ordering::Acquire);

        if current == target {
            SyncStatus::Synced {
                synced_da_height: current,
            }
        } else {
            SyncStatus::Syncing {
                synced_da_height: current,
                target_da_height: target,
            }
        }
    }
}

/// How [`StateTransitionRunner`] is initialized
pub enum InitVariant<
    Stf: StateTransitionFunction<InnerVm, OuterVm, Da::Spec>,
    InnerVm: Zkvm,
    OuterVm: Zkvm,
    Da: DaService,
> {
    /// From give state root
    Initialized(Stf::StateRoot),
    /// From empty state root
    Genesis {
        /// Genesis block header should be finalized at an initialization moment.
        block: Da::FilteredBlock,
        /// Genesis params for Stf::init.
        genesis_params: GenesisParams<Stf, InnerVm, OuterVm, Da::Spec>,
    },
}

impl<Stf, Sm, Da, InnerVm, OuterVm, Ps> StateTransitionRunner<Stf, Sm, Da, InnerVm, OuterVm, Ps>
where
    Da: DaService<Error = anyhow::Error> + Clone,
    InnerVm: ZkvmHost,
    OuterVm: ZkvmHost,
    Sm: HierarchicalStorageManager<Da::Spec, LedgerChangeSet = SchemaBatch, LedgerState = CacheDb>,
    Sm::StfState: Clone,
    Stf: StateTransitionFunction<
        <InnerVm::Guest as ZkvmGuest>::Verifier,
        <OuterVm::Guest as ZkvmGuest>::Verifier,
        Da::Spec,
        Condition = <Da::Spec as DaSpec>::ValidityCondition,
        PreState = Sm::StfState,
        ChangeSet = Sm::StfChangeSet,
    >,
    Ps: ProverService<StateRoot = Stf::StateRoot, Witness = Stf::Witness, DaService = Da>,
{
    /// Creates a new [`StateTransitionRunner`].
    ///
    /// If a previous state root is provided, it uses that as the starting point
    /// for execution. Otherwise, initializes the chain using the provided
    /// genesis config.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runner_config: RunnerConfig,
        da_service: Arc<Da>,
        mut ledger_db: LedgerDb,
        stf: Stf,
        mut storage_manager: Sm,
        rpc_storage_sender: watch::Sender<Sm::StfState>,
        init_variant: InitVariant<
            Stf,
            <InnerVm::Guest as ZkvmGuest>::Verifier,
            <OuterVm::Guest as ZkvmGuest>::Verifier,
            Da,
        >,
        proof_manager: ProofManager<Ps>,
    ) -> Result<Self, anyhow::Error> {
        let rpc_config = runner_config.rpc_config;
        let axum_config = runner_config.axum_config;

        let prev_state_root = match init_variant {
            InitVariant::Initialized(state_root) => {
                debug!("Chain is already initialized; skipping initialization");
                state_root
            }
            InitVariant::Genesis {
                block,
                genesis_params: params,
            } => {
                let block_header = block.header().clone();
                info!(
                    header = %block_header.display(),
                    "No history detected. Initializing chain on the block header..."
                );
                let (stf_state, ledger_state) = storage_manager.create_state_for(&block_header)?;
                ledger_db.replace_db(ledger_state)?;
                let (genesis_root, initialized_storage) = stf.init_chain(stf_state, params);
                let data_to_commit: SlotCommit<
                    _,
                    Stf::BatchReceiptContents,
                    Stf::TxReceiptContents,
                > = SlotCommit::new(block);
                let mut ledger_change_set =
                    ledger_db.materialize_slot(data_to_commit, genesis_root.as_ref())?;
                let finalized_slot_changes = ledger_db.materialize_latest_finalize_slot(0)?;
                ledger_change_set.merge(finalized_slot_changes);
                storage_manager.save_change_set(
                    &block_header,
                    initialized_storage,
                    ledger_change_set,
                )?;

                storage_manager.finalize(&block_header)?;
                ledger_db.send_notifications();
                info!(
                    genesis_root = hex::encode(genesis_root.as_ref()),
                    "Chain initialization is done"
                );
                genesis_root
            }
        };

        let listen_address_rpc =
            SocketAddr::new(rpc_config.bind_host.parse()?, rpc_config.bind_port);
        let listen_address_axum =
            SocketAddr::new(axum_config.bind_host.parse()?, axum_config.bind_port);

        // Start the main rollup loop
        let next_item_numbers = ledger_db.get_next_items_numbers()?;
        let last_slot_processed_before_shutdown = next_item_numbers.slot_number.saturating_sub(1);

        let da_height_processed =
            runner_config.genesis_height + last_slot_processed_before_shutdown;

        let first_unprocessed_height_at_startup = da_height_processed + 1;
        debug!(%last_slot_processed_before_shutdown, %runner_config.genesis_height, %first_unprocessed_height_at_startup, "Initializing StfRunner");

        let state_manager = StateManager::new(
            storage_manager,
            ledger_db,
            prev_state_root,
            rpc_storage_sender,
        );

        Ok(Self {
            first_unprocessed_height_at_startup,
            da_polling_interval_ms: runner_config.da_polling_interval_ms,
            da_service: da_service.clone(),
            stf,
            state_manager,
            listen_address_rpc,
            listen_address_axum,
            proof_manager,

            sync_state: Arc::new(DaSyncState {
                synced_da_height: AtomicU64::new(da_height_processed),
                target_da_height: AtomicU64::new(u64::MAX),
            }),
        })
    }

    /// Starts an RPC server with provided rpc methods.
    ///  # Arguments:
    ///   * methods: [`RpcModule`] with all RPC methods.
    ///   * channel: If `Some`, notification with actual [`SocketAddr`] where RPC server listens to.
    pub async fn start_rpc_server(&self, methods: RpcModule<()>) -> anyhow::Result<SocketAddr> {
        let server = jsonrpsee::server::ServerBuilder::default()
            .build([self.listen_address_rpc].as_ref())
            .await?;
        let rpc_address = server.local_addr()?;

        let _handle = tokio::spawn(async move {
            info!(%rpc_address, "Starting RPC server");
            let _server_handle = server.start(methods);

            futures::future::pending::<()>().await;
        });

        Ok(rpc_address)
    }

    /// Starts an Axum server with the provided router.
    pub async fn start_axum_server(&self, router: axum::Router<()>) -> anyhow::Result<SocketAddr> {
        let listener = tokio::net::TcpListener::bind(self.listen_address_axum).await?;
        let rest_address = listener.local_addr()?;

        tokio::spawn(async move {
            info!(%rest_address, "Starting REST API server");
            axum::serve(listener, router).await.unwrap();
        });

        Ok(rest_address)
    }

    /// Spawn a [`tokio::task`] that updates the sync status every `polling_interval`.
    pub fn spawn_sync_status_updater(&self, polling_interval: Duration) {
        let sync_state = self.sync_state.clone();
        let da_service = self.da_service.clone();

        tokio::task::spawn(async move {
            let mut interval = tokio::time::interval(polling_interval);
            debug!(?interval, "Interval for polling sync da height");
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            interval.tick().await; // Tick the interval once because it starts at 0ms. <https://docs.rs/tokio/latest/src/tokio/time/interval.rs.html#427>

            loop {
                if let Err(error) = sync_state.update_target(da_service.as_ref()).await {
                    error!(
                        ?error,
                        "Failed to update the sync status; will retry in ~{}ms",
                        polling_interval.as_millis()
                    );
                }

                if let SyncStatus::Syncing {
                    synced_da_height,
                    target_da_height,
                } = sync_state.status()
                {
                    info!(synced_da_height, target_da_height, "Sync in progress");
                }
                interval.tick().await;
            }
        });
    }

    /// Runs the rollup.
    pub async fn run_in_process(&mut self) -> anyhow::Result<()> {
        let mut next_da_height = self.first_unprocessed_height_at_startup;
        let target_height = self.da_service.get_head_block_header().await?.height();
        self.sync_state
            .target_da_height
            .store(target_height, std::sync::atomic::Ordering::Release);

        self.spawn_sync_status_updater(Duration::from_millis(self.da_polling_interval_ms));

        loop {
            debug!(
                next_da_height,
                current_state_root = hex::encode(self.get_state_root().as_ref()),
                "Requesting DA block"
            );
            sov_metrics::update_metrics(|metrics| {
                metrics.current_da_height.set(next_da_height as i64);
            });

            let mut transaction_count = 0;
            let mut batch_count = 0;
            let filtered_block = self.da_service.get_block_at(next_da_height).await?;
            debug!(header = %filtered_block.header().display(), "Fetched block header");

            let (stf_pre_state, filtered_block) = self
                .state_manager
                .prepare_storage(filtered_block, &self.da_service)
                .await?;

            let filtered_block_header = filtered_block.header().clone();
            if next_da_height != filtered_block_header.height() {
                debug!(
                    existing_next_da_height = next_da_height,
                    new_next_da_height = filtered_block_header.height(),
                    "Updating next_da_height after storage_manager "
                );
                next_da_height = filtered_block_header.height();
            }

            // STF execution
            let mut relevant_blobs = self.da_service.extract_relevant_blobs(&filtered_block);
            let batch_blobs = &mut relevant_blobs.batch_blobs;
            let proof_blobs = &relevant_blobs.proof_blobs;
            info!(
                batch_blobs_count = batch_blobs.len(),
                next_da_height,
                batch_blobs = ?batch_blobs
                    .iter()
                    .map(|b| format!(
                        "sequencer={} blob_hash=0x{}",
                        b.sender(),
                        hex::encode(b.hash())
                    ))
                    .collect::<Vec<_>>(),
                proof_blobs = ?proof_blobs
                    .iter()
                    .map(|b| format!(
                        "sequencer={} blob_hash=0x{}",
                        b.sender(),
                        hex::encode(b.hash())
                    ))
                    .collect::<Vec<_>>(),
                "Extracted relevant blobs"
            );

            let slot_result = self.stf.apply_slot(
                self.state_manager.get_state_root(),
                stf_pre_state,
                Default::default(),
                &filtered_block_header,
                &filtered_block.validity_condition(),
                relevant_blobs.as_iters(),
            );

            // Getting relevant proofs
            let relevant_proofs = self
                .da_service
                .get_extraction_proof(&filtered_block, &relevant_blobs)
                .await;

            // Handling executed data
            let mut data_to_commit = SlotCommit::new(filtered_block);
            for receipt in slot_result.batch_receipts {
                batch_count += 1;
                transaction_count += receipt.tx_receipts.len();
                data_to_commit.add_batch(receipt);
            }
            let transition_data: StateTransitionWitness<Stf::StateRoot, Stf::Witness, Da::Spec> =
                StateTransitionWitness {
                    initial_state_root: self.get_state_root().clone(),
                    final_state_root: slot_result.state_root.clone(),
                    da_block_header: filtered_block_header.clone(),
                    relevant_proofs,
                    relevant_blobs,
                    witness: slot_result.witness,
                };
            let zk_proofs_from_stf = slot_result
                .proof_receipts
                .into_iter()
                .map(|proof_receipt| proof_receipt.raw_proof);
            let aggregated_proofs = self
                .proof_manager
                .verify_aggregated_proofs(zk_proofs_from_stf)
                .await?;

            // Processing finalized headers.
            let last_finalized = self.da_service.get_last_finalized_block_header().await?;
            debug!(header = %last_finalized.display(), "Got last finalized header");
            let last_finalized_height = last_finalized.height();

            let finalized_transitions = self
                .state_manager
                .process_stf_changes(
                    last_finalized_height,
                    slot_result.change_set,
                    transition_data,
                    data_to_commit,
                    aggregated_proofs,
                )
                .await?;

            // TODO: We are now submitting proofs after they has been saved, not before
            //   so need to test a restart and submitting non submitted proofs.
            self.process_finalized_state_transitions(finalized_transitions)
                .await?;

            // Updating counters and metrics
            self.sync_state
                .synced_da_height
                .store(next_da_height, std::sync::atomic::Ordering::Release);
            debug!(
                height = next_da_height,
                state_root = hex::encode(self.get_state_root().as_ref()),
                "Execution of block is completed"
            );
            next_da_height += 1;

            sov_metrics::update_metrics(|metrics| {
                metrics.rollup_batches_processed.inc_by(batch_count);
                metrics.rollup_txns_processed.inc_by(transaction_count as _);
            });
        }
    }

    /// Post proofs for finalized state transitions
    async fn process_finalized_state_transitions(
        &mut self,
        finalized_transitions: Vec<StateTransitionInfo<Stf::StateRoot, Stf::Witness, Da::Spec>>,
    ) -> anyhow::Result<()> {
        for transition_data in finalized_transitions {
            // Post ZK proof to DA.
            self.proof_manager
                .post_aggregated_proof_to_da_when_ready(transition_data)
                .await?;
        }
        Ok(())
    }

    /// Allows reading current state root
    pub fn get_state_root(&self) -> &Stf::StateRoot {
        self.state_manager.get_state_root()
    }

    /// Retrieve a handle for the underlying DA service
    pub fn da_service(&self) -> Arc<Da> {
        self.da_service.clone()
    }
}
