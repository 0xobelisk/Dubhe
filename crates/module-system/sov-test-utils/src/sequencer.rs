use std::net::SocketAddr;

use sov_db::schema::SchemaBatch;
use sov_kernels::basic::{BasicKernel, BasicKernelGenesisConfig};
use sov_mock_da::{MockBlockHeader, MockDaService, MockDaSpec};
use sov_mock_zkvm::MockCodeCommitment;
use sov_modules_api::{Address, CryptoSpec, PrivateKey, Spec};
use sov_modules_stf_blueprint::{GenesisParams, StfBlueprint};
use sov_prover_storage_manager::ProverStorageManager;
use sov_rollup_interface::services::batch_builder::BatchBuilder;
use sov_rollup_interface::stf::StateTransitionFunction;
use sov_rollup_interface::storage::HierarchicalStorageManager;
use sov_sequencer::{FairBatchBuilder, FairBatchBuilderConfig, Sequencer, SequencerDb};
use sov_sequencer_json_client::Client;
use sov_state::DefaultStorageSpec;
use tempfile::TempDir;
use tokio::sync::watch;

use crate::auth::TestAuth;
use crate::runtime::optimistic::{create_genesis_config, TestRuntime};
use crate::runtime::ChainStateConfig;
use crate::{TestHasher, TestPrivateKey, TestSpec};

const SEQUENCER_ADDR: [u8; 32] = [42u8; 32];

pub type Blueprint = StfBlueprint<
    TestSpec,
    MockDaSpec,
    TestRuntime<TestSpec, MockDaSpec>,
    BasicKernel<TestSpec, MockDaSpec>,
>;

pub type TestSequencer<B> = Sequencer<B, MockDaService, TestAuth<TestSpec, MockDaSpec>>;

pub type TestCryptoSpec = <TestSpec as Spec>::CryptoSpec;
pub type AdminPrivateKey = <TestCryptoSpec as CryptoSpec>::PrivateKey;

pub type TestFairBatchBuilder = FairBatchBuilder<
    TestSpec,
    MockDaSpec,
    TestRuntime<TestSpec, MockDaSpec>,
    BasicKernel<TestSpec, MockDaSpec>,
    TestAuth<TestSpec, MockDaSpec>,
>;

/// A `struct` that contains a [`Sequencer`] and a copy of its running Axum
/// server, for use in tests. See [`TestSequencerSetup::new`] and
/// [`TestSequencerSetup::with_real_batch_builder`].
pub struct TestSequencerSetup<B: BatchBuilder> {
    _dir: TempDir,
    pub da_service: MockDaService,
    pub sequencer: TestSequencer<B>,
    pub admin_private_key: AdminPrivateKey,
    pub axum_server_handle: axum_server::Handle,
    pub axum_addr: SocketAddr,
}

impl<B: BatchBuilder> Drop for TestSequencerSetup<B> {
    fn drop(&mut self) {
        self.axum_server_handle.shutdown();
    }
}

impl<B> TestSequencerSetup<B>
where
    B: BatchBuilder + Send + Sync + 'static,
{
    /// Instantiates a new [`Sequencer`] with a [`TestRuntime`] and an empty
    /// [`MockDaService`].
    ///
    /// The RPC and Axum servers for the newly generated [`Sequencer`] are created
    /// on the fly, and their handles are stored inside a [`TestSequencerSetup`].
    /// This results in the automatic shutdown of the servers when the
    /// [`TestSequencerSetup`] is dropped.
    pub async fn new(
        dir: TempDir,
        da_service: MockDaService,
        batch_builder: B,
    ) -> anyhow::Result<Self> {
        // Use "same" bytes for sequencer address and rollup address.
        let sequencer_rollup_addr = Address::from(SEQUENCER_ADDR);
        let admin_pkey = TestPrivateKey::generate();
        let runtime = TestRuntime::<TestSpec, MockDaSpec>::default();

        let storage_config = sov_state::config::Config {
            path: dir.path().to_path_buf(),
        };
        let mut storage_manager =
            ProverStorageManager::<MockDaSpec, DefaultStorageSpec<TestHasher>>::new(
                storage_config,
            )?;
        let genesis_block_header = MockBlockHeader::from_height(0);
        let (stf_state, _) = storage_manager.create_state_for(&genesis_block_header)?;

        let genesis_config = create_genesis_config(
            (&admin_pkey.pub_key()).into(),
            &[],
            sequencer_rollup_addr,
            SEQUENCER_ADDR.into(),
            100_000_000,
            "SovereignToken".to_string(),
            1_000_000_000,
        );

        let kernel_genesis = BasicKernelGenesisConfig {
            chain_state: ChainStateConfig {
                current_time: Default::default(),
                inner_code_commitment: MockCodeCommitment::default(),
                outer_code_commitment: MockCodeCommitment::default(),
                genesis_da_height: 0,
            },
        };
        let params = GenesisParams {
            runtime: genesis_config,
            kernel: kernel_genesis,
        };

        let blueprint = Blueprint::with_runtime(runtime.clone());
        let (_root_hash, change_set) = blueprint.init_chain(stf_state, params);

        storage_manager.save_change_set(&genesis_block_header, change_set, SchemaBatch::new())?;

        let sequencer = Sequencer::new(batch_builder, da_service.clone());

        let (axum_addr, sequencer_axum_server) = {
            let addr = SocketAddr::from(([127, 0, 0, 1], 0));
            let router = sequencer
                .axum_router("/sequencer")
                .with_state::<()>(sequencer.clone());

            let handle = axum_server::Handle::new();
            let handle1 = handle.clone();
            tokio::spawn(async move {
                axum_server::Server::bind(addr)
                    .handle(handle1)
                    .serve(router.into_make_service())
                    .await
                    .unwrap();
            });

            (handle.listening().await.unwrap(), handle)
        };

        Ok(Self {
            _dir: dir,
            da_service,
            sequencer,
            admin_private_key: admin_pkey,
            axum_server_handle: sequencer_axum_server,
            axum_addr,
        })
    }

    pub fn client(&self) -> Client {
        Client::new(&format!("http://{}", self.axum_addr))
    }
}

impl TestSequencerSetup<TestFairBatchBuilder> {
    pub async fn with_real_batch_builder() -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;

        // Use "same" bytes for sequencer address and rollup address.
        let sequencer_rollup_addr = Address::from(SEQUENCER_ADDR);
        let admin_pkey = TestPrivateKey::generate();
        let runtime = TestRuntime::<TestSpec, MockDaSpec>::default();

        let storage_config = sov_state::config::Config {
            path: dir.path().to_path_buf(),
        };
        let mut storage_manager =
            ProverStorageManager::<MockDaSpec, DefaultStorageSpec<TestHasher>>::new(
                storage_config,
            )?;
        let genesis_block_header = MockBlockHeader::from_height(0);
        let (stf_state, _) = storage_manager.create_state_for(&genesis_block_header)?;

        let genesis_config = create_genesis_config(
            (&admin_pkey.pub_key()).into(),
            &[],
            sequencer_rollup_addr,
            SEQUENCER_ADDR.into(),
            100_000_000,
            "SovereignToken".to_string(),
            1_000_000_000,
        );

        let kernel_genesis = BasicKernelGenesisConfig {
            chain_state: ChainStateConfig {
                current_time: Default::default(),
                inner_code_commitment: MockCodeCommitment::default(),
                outer_code_commitment: MockCodeCommitment::default(),
                genesis_da_height: 0,
            },
        };
        let params = GenesisParams {
            runtime: genesis_config,
            kernel: kernel_genesis,
        };

        let blueprint = Blueprint::with_runtime(runtime.clone());
        let (_root_hash, change_set) = blueprint.init_chain(stf_state, params);

        storage_manager.save_change_set(&genesis_block_header, change_set, SchemaBatch::new())?;

        let first_block = MockBlockHeader::from_height(1);

        let sequencer_db = SequencerDb::new(dir.path())?;

        let (stf_state, _ledger_storage) = storage_manager.create_state_for(&first_block)?;

        let batch_builder_config = FairBatchBuilderConfig {
            mempool_max_txs_count: usize::MAX,
            max_batch_size_bytes: usize::MAX,
            sequencer_address: SEQUENCER_ADDR.into(),
        };
        let batch_builder = FairBatchBuilder::new(
            runtime,
            BasicKernel::default(),
            watch::Sender::new(stf_state).subscribe(),
            sequencer_db,
            batch_builder_config,
        )?;

        let da_service = MockDaService::new(SEQUENCER_ADDR.into());
        let sequencer = Sequencer::new(batch_builder, da_service.clone());

        let (axum_addr, sequencer_axum_server) = {
            let addr = SocketAddr::from(([127, 0, 0, 1], 0));
            let router = sequencer
                .axum_router("/sequencer")
                .with_state::<()>(sequencer.clone());

            let handle = axum_server::Handle::new();
            let handle1 = handle.clone();
            tokio::spawn(async move {
                axum_server::Server::bind(addr)
                    .handle(handle1)
                    .serve(router.into_make_service())
                    .await
                    .unwrap();
            });

            (handle.listening().await.unwrap(), handle)
        };

        Ok(Self {
            _dir: dir,
            da_service,
            sequencer,
            admin_private_key: admin_pkey,
            axum_server_handle: sequencer_axum_server,
            axum_addr,
        })
    }
}
