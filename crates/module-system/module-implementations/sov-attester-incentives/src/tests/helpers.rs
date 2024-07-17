use std::convert::Infallible;

use sov_bank::{Bank, BankConfig, GasTokenConfig, IntoPayable, ReserveGasError};
use sov_chain_state::ChainState;
use sov_mock_da::{MockBlock, MockBlockHeader, MockDaSpec, MockValidityCond};
use sov_modules_api::optimistic::Attestation;
use sov_modules_api::runtime::capabilities::mocks::MockKernel;
use sov_modules_api::transaction::{PriorityFeeBips, Transaction, TxDetails};
use sov_modules_api::utils::generate_address;
use sov_modules_api::{
    ApiStateAccessor, CryptoSpec, GasArray, GasMeter, Genesis, KernelModule, KernelWorkingSet,
    ModuleInfo, PrivateKey, Spec, StateCheckpoint, UnlimitedGasMeter,
};
use sov_prover_storage_manager::SimpleStorageManager;
use sov_rollup_interface::da::Time;
use sov_state::namespaces::User;
use sov_state::storage::{NativeStorage, Storage, StorageProof};
use sov_state::{ProverStorage, SparseMerkleProof, StorageRoot};
use sov_test_utils::{
    TestStorageSpec as StorageSpec, TEST_DEFAULT_GAS_LIMIT, TEST_DEFAULT_MAX_FEE,
    TEST_DEFAULT_USER_BALANCE, TEST_DEFAULT_USER_STAKE,
};

use crate::AttesterIncentives;

type S = sov_test_utils::TestSpec;

pub const TOKEN_NAME: &str = "TEST_TOKEN";
pub const DEFAULT_ROLLUP_FINALITY: u64 = 3;
pub const INIT_HEIGHT: u64 = 0;

pub const NUM_BANK_ACCOUNTS: usize = 3;

/// Consumes and commit the existing working set on the underlying storage
/// `storage` must be the underlying storage defined on the working set for this method to work.
pub(crate) fn commit_get_new_storage(
    storage: ProverStorage<StorageSpec>,
    checkpoint: StateCheckpoint<S>,
    storage_manager: &mut SimpleStorageManager<StorageSpec>,
) -> (StorageRoot<StorageSpec>, ProverStorage<StorageSpec>) {
    let (reads_writes, _, witness) = checkpoint.freeze();

    let (new_root, change_set) = storage
        .validate_and_materialize(reads_writes, &witness)
        .expect("Should be able to materialize changes");

    storage_manager.commit(change_set);

    (new_root, storage_manager.create_storage())
}

pub(crate) fn create_bank_config_with_token(
    token_name: String,
    addresses_count: usize,
    initial_balance: u64,
) -> (BankConfig<S>, Vec<<S as Spec>::Address>) {
    let address_and_balances: Vec<(<S as Spec>::Address, u64)> = (0..addresses_count)
        .map(|i| {
            let key = format!("key_{}", i);
            let addr = generate_address::<S>(&key);
            (addr, initial_balance)
        })
        .collect();

    let token_config = GasTokenConfig {
        token_name,
        address_and_balances: address_and_balances.clone(),
        authorized_minters: vec![address_and_balances.first().unwrap().0],
    };

    (
        BankConfig {
            gas_token_config: token_config,
            tokens: vec![],
        },
        address_and_balances
            .into_iter()
            .map(|(addr, _)| addr)
            .collect(),
    )
}

/// Creates a bank config with a token, and a prover incentives module.
/// Returns the prover incentives module and the attester and challenger's addresses.
#[allow(clippy::type_complexity)]
pub(crate) fn setup(
    state: StateCheckpoint<S>,
) -> (
    AttesterIncentives<S, MockDaSpec>,
    <S as Spec>::Address,
    <S as Spec>::Address,
    <S as Spec>::Address,
    StateCheckpoint<S>,
) {
    // Initialize bank
    let (bank_config, mut addresses) = create_bank_config_with_token(
        TOKEN_NAME.to_string(),
        NUM_BANK_ACCOUNTS,
        TEST_DEFAULT_USER_BALANCE,
    );
    let bank = sov_bank::Bank::<S>::default();
    let mut genesis_state = state.to_genesis_state_accessor::<Bank<S>>(&bank_config);
    bank.genesis(&bank_config, &mut genesis_state)
        .expect("bank genesis must succeed");
    let mut state = genesis_state.checkpoint();

    let attester_address = addresses.pop().unwrap();
    let challenger_address = addresses.pop().unwrap();
    let sequencer = addresses.pop().unwrap();

    // Initialize chain state
    let chain_state_config = sov_chain_state::ChainStateConfig {
        current_time: Default::default(),
        genesis_da_height: 0,
        inner_code_commitment: Default::default(),
        outer_code_commitment: Default::default(),
    };

    let chain_state = sov_chain_state::ChainState::<S, MockDaSpec>::default();
    chain_state
        .genesis_unchecked(
            &chain_state_config,
            &mut KernelWorkingSet::uninitialized(&mut state),
        )
        .expect("Chain state genesis must succeed");

    // initialize prover incentives
    let module = AttesterIncentives::<S, MockDaSpec>::default();
    let config = crate::AttesterIncentivesConfig {
        minimum_attester_bond: TEST_DEFAULT_USER_STAKE,
        minimum_challenger_bond: TEST_DEFAULT_USER_STAKE,
        initial_attesters: vec![(attester_address, TEST_DEFAULT_USER_STAKE)],
        rollup_finality_period: DEFAULT_ROLLUP_FINALITY,
        maximum_attested_height: INIT_HEIGHT,
        light_client_finalized_height: INIT_HEIGHT,
        phantom_data: Default::default(),
    };
    let mut genesis_state =
        state.to_genesis_state_accessor::<AttesterIncentives<S, MockDaSpec>>(&config);

    module
        .genesis(&config, &mut genesis_state)
        .expect("prover incentives genesis must succeed");
    let state = genesis_state.checkpoint();

    (
        module,
        attester_address,
        challenger_address,
        sequencer,
        state,
    )
}

pub(crate) struct ExecutionSimulationVars {
    pub state_root: StorageRoot<StorageSpec>,
    pub state_proof:
        StorageProof<SparseMerkleProof<<<S as Spec>::CryptoSpec as CryptoSpec>::Hasher>>,
}

impl ExecutionSimulationVars {
    /// Generate an execution simulation for a given number of rounds. Returns a list of the successive state roots
    /// with associated bonding proofs.
    /// The execution simulation also performs a gas charge and refund for the sequencer, which locks reward to the attester module.
    pub(crate) fn execute(
        rounds: u8,
        module: &AttesterIncentives<S, MockDaSpec>,
        storage_manager: &mut SimpleStorageManager<StorageSpec>,
        sequencer: &<S as Spec>::Address,
        attester_address: &<S as Spec>::Address,
    ) -> (
        // Vector of the successive state roots with associated bonding proofs
        Vec<Self>,
        StateCheckpoint<S>,
    ) {
        let mut ret_exec_vars = Vec::<ExecutionSimulationVars>::new();

        let mut storage = storage_manager.create_storage();
        let mut state_checkpoint = StateCheckpoint::new(storage.clone());

        for i in 0..rounds {
            // Commit the working set
            let (root_hash, new_storage) =
                commit_get_new_storage(storage, state_checkpoint, storage_manager);
            storage = new_storage;
            state_checkpoint = StateCheckpoint::new(storage.clone());

            let bond_proof = storage
                .get_with_proof::<User>(module.get_attester_storage_key(*attester_address), None);

            // Then process the first transaction. Only sets the genesis hash and a transition in progress.
            let slot_data = MockBlock {
                header: MockBlockHeader {
                    prev_hash: [i; 32].into(),
                    hash: [i + 1; 32].into(),
                    height: INIT_HEIGHT + u64::from(i + 1),
                    time: Time::now(),
                },
                validity_cond: MockValidityCond { is_valid: true },
                batch_blobs: Default::default(),
                proof_blobs: Default::default(),
            };
            let kernel = MockKernel::<S, MockDaSpec>::new(i as u64, i as u64);

            // We also need to call the `GasEnforcer` hook to ensure that the reward pool is populated.
            let tx_key = <<S as Spec>::CryptoSpec as CryptoSpec>::PrivateKey::generate();

            let tx = Transaction::<S>::new_with_details(
                tx_key.pub_key(),
                vec![],
                tx_key.sign(&[]),
                i.into(),
                TxDetails {
                    max_priority_fee_bips: PriorityFeeBips::ZERO,
                    max_fee: TEST_DEFAULT_MAX_FEE,
                    gas_limit: Some(<S as Spec>::Gas::from_slice(&TEST_DEFAULT_GAS_LIMIT)),
                    chain_id: 0,
                },
            )
            .into();

            let mut kernel_working_set =
                KernelWorkingSet::from_kernel(&kernel, &mut state_checkpoint);

            // We execute the chain state to make sure the transition data is persisted
            let _current_base_fee_per_gas = module.chain_state.begin_slot_hook(
                &slot_data.header,
                &slot_data.validity_cond,
                &root_hash,
                &mut kernel_working_set,
            );

            let transaction_scratchpad = state_checkpoint.to_tx_scratchpad();

            let pre_exec_working_set = transaction_scratchpad.pre_exec_ws_unmetered();

            // We first need to reserve gas for the transaction
            let mut state = match module
                .bank
                .reserve_gas(&tx, sequencer, pre_exec_working_set)
            {
                Ok(ws) => ws,
                Err(ReserveGasError::<S, UnlimitedGasMeter<<S as Spec>::Gas>> {
                    pre_exec_working_set: _,
                    reason,
                }) => {
                    panic!("Unable to reserve gas for the transaction in the execution simulation: {:?}", reason);
                }
            };

            // We charge some gas to the sequencer to make sure the gas meter is updated
            state
                .charge_gas(&<S as Spec>::Gas::from_slice(&TEST_DEFAULT_GAS_LIMIT))
                .expect("Gas charge failed");

            let (mut tx_scratchpad, tx_consumption, _) = state.finalize();

            module.bank.allocate_consumed_gas(
                &module.id().to_payable(),
                &module.id().to_payable(),
                &tx_consumption,
                &mut tx_scratchpad,
            );

            // Then we can refund some gas to the sequencer
            module
                .bank
                .refund_remaining_gas(sequencer, &tx_consumption, &mut tx_scratchpad);

            let mut checkpoint = tx_scratchpad.commit();

            ret_exec_vars.push(ExecutionSimulationVars {
                state_root: root_hash,
                state_proof: bond_proof,
            });

            let kernel = MockKernel::<S, MockDaSpec>::new((i + 1) as u64, (i + 1) as u64);
            let mut kernel_working_set = KernelWorkingSet::from_kernel(&kernel, &mut checkpoint);

            module
                .chain_state
                .end_slot_hook(tx_consumption.base_fee(), &mut kernel_working_set);

            state_checkpoint = checkpoint;
        }

        (ret_exec_vars, state_checkpoint)
    }
}

#[allow(clippy::type_complexity)]
pub fn create_attestation(
    slot_to_attest: u64,
    attester_address: &<S as Spec>::Address,
    state: &mut ApiStateAccessor<S>,
) -> Result<
    Attestation<
        MockDaSpec,
        StorageProof<<<S as Spec>::Storage as Storage>::Proof>,
        <<S as Spec>::Storage as Storage>::Root,
    >,
    Infallible,
> {
    let chain_state = ChainState::<S, MockDaSpec>::default();

    // Get the values for the transition being attested
    let current_transition = chain_state
        .get_historical_transitions(slot_to_attest, state)?
        .unwrap();

    let prev_root = if slot_to_attest == 1 {
        chain_state.get_genesis_hash(state)?
    } else {
        chain_state
            .get_historical_transitions(slot_to_attest - 1, state)?
            .map(|t| *t.post_state_root())
    }
    .unwrap();

    let mut archival_state = state.get_archival_at(slot_to_attest);

    let proof_of_bond = AttesterIncentives::<S, MockDaSpec>::default()
        .bonded_attesters
        .get_with_proof(attester_address, &mut archival_state);

    Ok(Attestation {
        initial_state_root: prev_root,
        slot_hash: *current_transition.slot_hash(),
        post_state_root: *current_transition.post_state_root(),
        proof_of_bond: sov_modules_api::optimistic::ProofOfBond {
            claimed_transition_num: slot_to_attest,
            proof: proof_of_bond,
        },
    })
}
