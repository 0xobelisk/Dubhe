use async_trait::async_trait;
use demo_stf::authentication::ModAuth;
use demo_stf::genesis_config::StorageConfig;
use demo_stf::runtime::Runtime;
use sov_db::ledger_db::LedgerDb;
use sov_kernels::basic::BasicKernel;
use sov_mock_da::storable::service::StorableMockDaService;
use sov_mock_da::{MockDaConfig, MockDaSpec};
use sov_mock_zkvm::{MockCodeCommitment, MockZkVerifier, MockZkvm};
use sov_modules_api::default_spec::DefaultSpec;
use sov_modules_api::execution_mode::{ExecutionMode, Native, Zk};
use sov_modules_api::higher_kinded_types::Generic;
use sov_modules_api::{CryptoSpec, SovApiProofSerializer, Spec, Zkvm};
use sov_modules_rollup_blueprint::pluggable_traits::PluggableSpec;
use sov_modules_rollup_blueprint::{FullNodeBlueprint, RollupBlueprint};
use sov_modules_stf_blueprint::{RuntimeEndpoints, StfBlueprint};
use sov_prover_storage_manager::ProverStorageManager;
use sov_risc0_adapter::host::Risc0Host;
use sov_risc0_adapter::Risc0Verifier;
use sov_rollup_interface::services::da::DaServiceWithRetries;
use sov_rollup_interface::zk::aggregated_proof::CodeCommitment;
use sov_sequencer::SequencerDb;
use sov_state::{DefaultStorageSpec, Storage, ZkStorage};
use sov_stf_runner::{ParallelProverService, ProverService, RollupConfig, RollupProverConfig};
use tokio::sync::watch;

/// Rollup with MockDa
#[derive(Default)]
pub struct MockDemoRollup<M> {
    phantom: std::marker::PhantomData<M>,
}

impl<M: ExecutionMode> RollupBlueprint<M> for MockDemoRollup<M>
where
    DefaultSpec<Risc0Verifier, MockZkVerifier, M>: PluggableSpec,
{
    type Spec = DefaultSpec<Risc0Verifier, MockZkVerifier, M>;
    type Runtime = Runtime<Self::Spec, Self::DaSpec>;
    type DaSpec = MockDaSpec;
    type Kernel = BasicKernel<Self::Spec, Self::DaSpec>;
}

#[async_trait]
impl FullNodeBlueprint<Native> for MockDemoRollup<Native> {
    type DaService = DaServiceWithRetries<StorableMockDaService>;
    type DaConfig = MockDaConfig;
    type InnerZkvmHost = Risc0Host<'static>;
    type OuterZkvmHost = MockZkvm;

    type StorageManager = ProverStorageManager<
        MockDaSpec,
        DefaultStorageSpec<<<Self::Spec as Spec>::CryptoSpec as CryptoSpec>::Hasher>,
    >;

    type ProverService = ParallelProverService<
        <Self::Spec as Spec>::Address,
        <<Self::Spec as Spec>::Storage as Storage>::Root,
        <<Self::Spec as Spec>::Storage as Storage>::Witness,
        Self::DaService,
        Self::InnerZkvmHost,
        Self::OuterZkvmHost,
        StfBlueprint<
            <Self::Spec as Generic>::With<Zk>,
            Self::DaSpec,
            <MockDemoRollup<Zk> as RollupBlueprint<Zk>>::Runtime,
            <MockDemoRollup<Zk> as RollupBlueprint<Zk>>::Kernel,
        >,
    >;

    type ProofSerializer = SovApiProofSerializer<Self::Spec>;

    fn create_outer_code_commitment(
        &self,
    ) -> <<Self::ProverService as ProverService>::Verifier as Zkvm>::CodeCommitment {
        MockCodeCommitment::default()
    }

    fn create_endpoints(
        &self,
        storage: watch::Receiver<<Self::Spec as Spec>::Storage>,
        ledger_db: &LedgerDb,
        sequencer_db: &SequencerDb,
        da_service: &Self::DaService,
        rollup_config: &RollupConfig<<Self::Spec as Spec>::Address, Self::DaConfig>,
    ) -> anyhow::Result<RuntimeEndpoints> {
        let mut endpoints = sov_modules_rollup_blueprint::register_endpoints::<
            Self,
            Native,
            ModAuth<Self::Spec, Self::DaSpec>,
        >(
            storage.clone(),
            ledger_db,
            sequencer_db,
            da_service,
            rollup_config.da.sender_address,
        )?;

        // TODO: Add issue for Sequencer level RPC injection:
        //   https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/366
        crate::eth::register_ethereum::<Self::Spec, Self::DaService>(
            da_service.clone(),
            storage,
            &mut endpoints.jsonrpsee_module,
        )?;

        Ok(endpoints)
    }

    async fn create_da_service(
        &self,
        rollup_config: &RollupConfig<<Self::Spec as Spec>::Address, Self::DaConfig>,
    ) -> Self::DaService {
        DaServiceWithRetries::new_fast(
            StorableMockDaService::from_config(rollup_config.da.clone()).await,
        )
    }

    async fn create_prover_service(
        &self,
        prover_config: RollupProverConfig,
        rollup_config: &RollupConfig<<Self::Spec as Spec>::Address, Self::DaConfig>,
        _da_service: &Self::DaService,
    ) -> Self::ProverService {
        let inner_vm = Risc0Host::new(risc0::MOCK_DA_ELF);
        let outer_vm = MockZkvm::new_non_blocking();
        let zk_stf = StfBlueprint::new();
        let zk_storage = ZkStorage::new();
        let da_verifier = Default::default();

        ParallelProverService::new_with_default_workers(
            inner_vm,
            outer_vm,
            zk_stf,
            da_verifier,
            prover_config,
            zk_storage,
            CodeCommitment::default(),
            rollup_config.proof_manager.prover_address,
        )
    }

    fn create_storage_manager(
        &self,
        rollup_config: &RollupConfig<<Self::Spec as Spec>::Address, Self::DaConfig>,
    ) -> anyhow::Result<Self::StorageManager> {
        let storage_config = StorageConfig {
            path: rollup_config.storage.path.clone(),
        };
        ProverStorageManager::new(storage_config)
    }
}
