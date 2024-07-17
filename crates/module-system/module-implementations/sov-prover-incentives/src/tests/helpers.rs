use sov_bank::{IntoPayable, ReserveGasError};
use sov_chain_state::StateTransition;
use sov_mock_da::{MockAddress, MockBlockHeader, MockDaSpec, MockHash, MockValidityCond};
use sov_mock_zkvm::{MockCodeCommitment, MockZkVerifier};
use sov_modules_api::capabilities::mocks::MockKernel;
use sov_modules_api::da::Time;
use sov_modules_api::digest::Digest;
use sov_modules_api::execution_mode::Native;
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::transaction::{Transaction, TxDetails};
use sov_modules_api::{
    Address, CryptoSpec, GasArray, GasMeter, InfallibleStateAccessor, KernelModule,
    KernelWorkingSet, Module, ModuleInfo, PrivateKey, Spec, StateAccessor, StateCheckpoint,
    StateReader,
};
use sov_prover_storage_manager::new_orphan_storage;
use sov_state::jmt::RootHash;
use sov_state::{DefaultStorageSpec, StorageRoot, User};
use sov_test_utils::{TEST_DEFAULT_MAX_FEE, TEST_DEFAULT_USER_BALANCE, TEST_DEFAULT_USER_STAKE};

use crate::ProverIncentives;

pub(crate) type S =
    sov_modules_api::default_spec::DefaultSpec<MockZkVerifier, MockZkVerifier, Native>;
pub(crate) type Da = MockDaSpec;

pub(crate) const INITIAL_PROVER_BALANCE: u64 = TEST_DEFAULT_USER_BALANCE;
pub(crate) const INITIAL_SEQUENCER_BALANCE: u64 = TEST_DEFAULT_USER_BALANCE;
pub(crate) const MOCK_CODE_COMMITMENT: MockCodeCommitment = MockCodeCommitment([0u8; 32]);
pub(crate) const MOCK_PROVER_ADDRESS: MockAddress = MockAddress::new([1u8; 32]);

pub const MAX_TX_GAS_AMOUNT: u64 = TEST_DEFAULT_MAX_FEE;

impl ProverIncentives<S, Da> {
    pub fn get_bond_amount<Accessor: StateAccessor>(
        &self,
        address: <S as Spec>::Address,
        working_set: &mut Accessor,
    ) -> Result<u64, <Accessor as StateReader<User>>::Error> {
        Ok(self
            .bonded_provers
            .get(&address, working_set)?
            .unwrap_or_default())
    }
}

/// Generates an address by hashing the provided `key`.
pub fn generate_address(key: &str) -> <S as Spec>::Address {
    let hash: [u8; 32] =
        <<S as Spec>::CryptoSpec as CryptoSpec>::Hasher::digest(key.as_bytes()).into();
    Address::from(hash)
}

fn create_bank_config() -> (
    sov_bank::BankConfig<S>,
    <S as Spec>::Address,
    <S as Spec>::Address,
) {
    let prover_address = generate_address("prover_pub_key");
    let sequencer_address = generate_address("sequencer_pub_key");

    let token_config = sov_bank::GasTokenConfig {
        token_name: "InitialToken".to_owned(),
        address_and_balances: vec![
            (prover_address, INITIAL_PROVER_BALANCE),
            (sequencer_address, INITIAL_SEQUENCER_BALANCE),
        ],
        authorized_minters: vec![],
    };

    (
        sov_bank::BankConfig {
            gas_token_config: token_config,
            tokens: vec![],
        },
        prover_address,
        sequencer_address,
    )
}

/// Simulates the execution of the chain state by applying `steps` state transitions.
pub(crate) fn simulate_chain_state_execution(
    module: &ProverIncentives<S, Da>,
    sequencer: <S as Spec>::Address,
    steps: u8,
    max_gas_used_per_step: &<S as Spec>::Gas,
    mut state_checkpoint: StateCheckpoint<S>,
) -> (StateCheckpoint<S>, Vec<u64>) {
    let mut total_gas_used = vec![];
    for i in 0..steps {
        let slot_header = MockBlockHeader {
            prev_hash: MockHash([i * 10; 32]),
            hash: MockHash([(i + 1) * 10; 32]),
            height: u64::from(i),
            time: Time::now(),
        };
        // We also need to call the `GasEnforcer` hook to ensure that the reward pool is populated.
        let tx_key = <<S as Spec>::CryptoSpec as CryptoSpec>::PrivateKey::generate();

        let tx = Transaction::<S>::new_with_details(
            tx_key.pub_key(),
            vec![],
            tx_key.sign(&[]),
            i.into(),
            TxDetails {
                max_priority_fee_bips: 0.into(),
                max_fee: MAX_TX_GAS_AMOUNT,
                gas_limit: Some(max_gas_used_per_step.clone()),
                chain_id: 0,
            },
        )
        .into();

        let kernel: MockKernel<S, _> = MockKernel::<S, MockDaSpec>::new(i.into(), i.into());
        let mut kernel_working_set = KernelWorkingSet::from_kernel(&kernel, &mut state_checkpoint);
        let price = module.chain_state.begin_slot_hook(
            &slot_header,
            &MockValidityCond { is_valid: true },
            &StorageRoot::<DefaultStorageSpec<<<S as Spec>::CryptoSpec as CryptoSpec>::Hasher>>::new(RootHash([i; 32]), RootHash([i; 32])),
            &mut kernel_working_set,
        );

        let tx_scratchpad = state_checkpoint.to_tx_scratchpad();
        let pre_exec_working_set = tx_scratchpad.pre_exec_ws_unmetered_with_price(&price);

        // We first need to reserve gas for the transaction
        // `state_checkpoint` does not implement `Debug` so we cannot just call `expect` here.
        let mut working_set = match module
            .bank
            .reserve_gas(&tx, &sequencer, pre_exec_working_set)
        {
            Ok(ws) => ws,
            Err(ReserveGasError {
                pre_exec_working_set: _,
                reason,
            }) => {
                panic!("Unable to reserve gas for the transaction: {:?}", reason);
            }
        };

        // We charge some gas to the sequencer to make sure the gas meter is updated
        let mut gas_to_charge = max_gas_used_per_step.clone();
        gas_to_charge.scalar_division(2);

        working_set
            .charge_gas(&gas_to_charge)
            .expect("Gas charge failed");

        let (mut tx_scratchpad, tx_consumption, _) = working_set.finalize();

        module.bank.allocate_consumed_gas(
            &module.id().to_payable(),
            &module.id().to_payable(),
            &tx_consumption,
            &mut tx_scratchpad,
        );

        module
            .bank
            .refund_remaining_gas(&sequencer, &tx_consumption, &mut tx_scratchpad);

        let mut checkpoint = tx_scratchpad.commit();

        total_gas_used.push(tx_consumption.total_consumption());

        let kernel: MockKernel<S, _> =
            MockKernel::<S, MockDaSpec>::new((i + 1).into(), (i + 1).into());
        let mut kernel_working_set = KernelWorkingSet::from_kernel(&kernel, &mut checkpoint);
        module
            .chain_state
            .end_slot_hook(tx_consumption.base_fee(), &mut kernel_working_set);

        state_checkpoint = checkpoint;
    }

    (state_checkpoint, total_gas_used)
}

fn setup_helper(
    state: StateCheckpoint<S>,
) -> (
    ProverIncentives<S, Da>,
    <S as Spec>::Address,
    <S as Spec>::Address,
    StateCheckpoint<S>,
) {
    // Initialize bank

    let (bank_config, prover_address, sequencer) = create_bank_config();
    let bank = sov_bank::Bank::<S>::default();
    let mut state = state.to_genesis_state_accessor::<sov_bank::Bank<S>>(&bank_config);
    bank.genesis(&bank_config, &mut state)
        .expect("bank genesis must succeed");

    // Initialize chain state
    let chain_state_config = sov_chain_state::ChainStateConfig {
        current_time: Time::now(),
        genesis_da_height: 0,
        inner_code_commitment: MockCodeCommitment::default(),
        outer_code_commitment: MockCodeCommitment::default(),
    };

    let chain_state = sov_chain_state::ChainState::<S, Da>::default();

    let mut checkpoint = state.checkpoint();

    let mut kernel_working_set = KernelWorkingSet::uninitialized(&mut checkpoint);
    chain_state
        .genesis_unchecked(&chain_state_config, &mut kernel_working_set)
        .expect("chain state genesis must succeed");

    // initialize prover incentives
    let module = ProverIncentives::<S, Da>::default();
    let config = crate::ProverIncentivesConfig {
        proving_penalty: TEST_DEFAULT_USER_STAKE / 2,
        minimum_bond: TEST_DEFAULT_USER_STAKE,
        initial_provers: vec![(prover_address, TEST_DEFAULT_USER_STAKE)],
    };

    let mut state = checkpoint.to_genesis_state_accessor::<ProverIncentives<S, Da>>(&config);

    module
        .genesis(&config, &mut state)
        .expect("prover incentives genesis must succeed");

    let checkpoint = state.checkpoint();
    (module, prover_address, sequencer, checkpoint)
}

pub(crate) fn setup() -> (
    crate::ProverIncentives<S, sov_mock_da::MockDaSpec>,
    <S as Spec>::Address,
    <S as Spec>::Address,
    StateCheckpoint<S>,
) {
    let tmpdir = tempfile::tempdir().unwrap();
    let state = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());
    let (module, prover_address, sequencer, mut state) = setup_helper(state);

    // Assert that the prover has the correct bond amount before processing the proof
    assert_eq!(
        module
            .get_bond_amount(prover_address, &mut state)
            .expect("The working set should not run out of gas during setup"),
        TEST_DEFAULT_USER_STAKE
    );

    (module, prover_address, sequencer, state)
}

pub(crate) fn get_transition_unwrap(
    transition_num: u64,
    module: &ProverIncentives<S, Da>,
    state: &mut impl InfallibleStateAccessor,
) -> StateTransition<S, Da> {
    module
        .chain_state
        .get_historical_transitions(transition_num, state)
        .unwrap_infallible()
        .expect("transition must exist")
}
