use std::rc::Rc;

pub use api_client::ApiClient;
use serde::{Deserialize, Serialize};
use sov_bank::{Bank, BankConfig, GasTokenConfig, GAS_TOKEN_ID};
pub use sov_db::schema::SchemaBatch;
use sov_mock_da::verifier::MockDaSpec;
use sov_mock_da::{MockAddress, MockBlob};
pub use sov_mock_zkvm::MockZkVerifier;
use sov_modules_api::capabilities::Authenticator;
use sov_modules_api::macros::config_value;
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::transaction::{PriorityFeeBips, Transaction, TxDetails, UnsignedTransaction};
use sov_modules_api::utils::generate_address;
pub use sov_modules_api::EncodeCall;
use sov_modules_api::{
    Batch, CryptoSpec, DaSpec, GasArray, GasUnit, Module, RawTx, Spec, StateCheckpoint,
};
use sov_modules_stf_blueprint::{BatchReceipt, BlobData, StfBlueprint};
use sov_prover_storage_manager::new_orphan_storage;
use sov_rollup_interface::stf::TxReceiptContents;

use crate::runtime::BasicKernel;

mod api_client;
pub mod auth;
mod evm;
pub mod generators;
#[cfg(feature = "demo-stf")]
pub mod ledger_db;
pub mod logging;
pub mod runtime;
pub mod sequencer;

pub use evm::simple_smart_contract::SimpleStorageContract;
use sov_modules_api::PrivateKey;
use sov_rollup_interface::execution_mode::{Native, Zk};

pub type TestSpec =
    sov_modules_api::default_spec::DefaultSpec<MockZkVerifier, MockZkVerifier, Native>;
pub type ZkTestSpec =
    sov_modules_api::default_spec::DefaultSpec<MockZkVerifier, MockZkVerifier, Zk>;
pub type TestAddress = <TestSpec as Spec>::Address;
pub type TestPrivateKey = <<TestSpec as Spec>::CryptoSpec as CryptoSpec>::PrivateKey;
pub type TestPublicKey = <<TestSpec as Spec>::CryptoSpec as CryptoSpec>::PublicKey;
pub type TestSignature = <<TestSpec as Spec>::CryptoSpec as CryptoSpec>::Signature;
pub type TestHasher = <<TestSpec as Spec>::CryptoSpec as CryptoSpec>::Hasher;
pub type TestStorageSpec = sov_state::DefaultStorageSpec<TestHasher>;
pub type TestStfBlueprint<RT, S> = StfBlueprint<S, MockDaSpec, RT, BasicKernel<S, MockDaSpec>>;

// --- Blessed test parameters ---

// Blessed gas parameters
pub const TEST_DEFAULT_MAX_FEE: u64 = 100000000;
pub const TEST_DEFAULT_GAS_LIMIT: [u64; 2] = [1000000, 1000000];
// The default amount of tokens that should be staked by a user (prover, sequencer, etc.)
pub const TEST_DEFAULT_USER_STAKE: u64 = 100000000;
// The default amount of tokens that should be in the user's bank account
pub const TEST_DEFAULT_USER_BALANCE: u64 = 1000000000;
pub const TEST_DEFAULT_MAX_PRIORITY_FEE: PriorityFeeBips = PriorityFeeBips::from_percentage(0);
// --- End Blessed gas parameters (used for testing) ---

// Blessed rollup constants
// Constants used in the genesis configuration of the test runtime
pub const TEST_MAX_ATTESTED_HEIGHT: u64 = 0;
pub const TEST_LIGHT_CLIENT_FINALIZED_HEIGHT: u64 = 0;
pub const TEST_ROLLUP_FINALITY_PERIOD: u64 = 1;

/// An implementation of [`TxReceiptContents`] for testing. TestTxReceiptContents uses
/// a `u32` as the receipt contents.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct TestTxReceiptContents;

impl TxReceiptContents for TestTxReceiptContents {
    type Skipped = u32;
    type Reverted = u32;
    type Successful = u32;
}

/// Test helper: Generates an empty transaction with the given gas parameters.
pub fn generate_empty_tx(
    max_priority_fee_bips: PriorityFeeBips,
    max_fee: u64,
    gas_limit: Option<GasUnit<2>>,
) -> Transaction<TestSpec> {
    Transaction::new_signed_tx(
        &TestPrivateKey::generate(),
        UnsignedTransaction::new(vec![], 0, max_priority_fee_bips, max_fee, 0, gas_limit),
    )
}

/// Simple setup, initializes a bank with a sender having an initial balance.
/// This is a useful helper for tests that need to initialize a bank.
pub fn simple_bank_setup(
    initial_balance: u64,
) -> (
    <TestSpec as Spec>::Address,
    Bank<TestSpec>,
    StateCheckpoint<TestSpec>,
) {
    let bank = Bank::<TestSpec>::default();
    let tmpdir = tempfile::tempdir().unwrap();
    let state_checkpoint = StateCheckpoint::new(new_orphan_storage(tmpdir.path()).unwrap());

    let sender_address = generate_address::<TestSpec>("just_sender");

    let token_name = "Token1".to_owned();
    let token_id = GAS_TOKEN_ID;

    let bank_config = BankConfig::<TestSpec> {
        gas_token_config: GasTokenConfig {
            token_name,
            address_and_balances: vec![(sender_address, initial_balance)],
            authorized_minters: vec![],
        },
        tokens: vec![],
    };
    let mut genesis_state_accessor =
        state_checkpoint.to_genesis_state_accessor::<Bank<TestSpec>>(&bank_config);
    bank.genesis(&bank_config, &mut genesis_state_accessor)
        .unwrap();

    let mut checkpoint = genesis_state_accessor.checkpoint();

    assert_eq!(
        bank.get_balance_of(&sender_address, token_id, &mut checkpoint)
            .unwrap_infallible(),
        Some(initial_balance),
        "Invalid initial balance"
    );

    (sender_address, bank, checkpoint)
}

pub fn new_test_blob_from_batch(
    batch: Batch,
    address: &[u8],
    hash: [u8; 32],
) -> <MockDaSpec as DaSpec>::BlobTransaction {
    let batch = BlobData::Batch(batch);
    let address = MockAddress::try_from(address).unwrap();
    let data = borsh::to_vec(&batch).unwrap();
    MockBlob::new(data, address, hash)
}

pub fn has_tx_events(apply_blob_outcome: &BatchReceipt) -> bool {
    let events = apply_blob_outcome
        .tx_receipts
        .iter()
        .flat_map(|receipts| receipts.events.iter());

    events.peekable().peek().is_some()
}

/// A generic message object used to create transactions.
pub struct Message<S: Spec, Mod: Module> {
    /// The sender's private key.
    pub sender_key: Rc<<S::CryptoSpec as CryptoSpec>::PrivateKey>,
    /// The message content.
    pub content: Mod::CallMessage,
    /// Data related to fees and gas handling.
    pub details: TxDetails<S>,
    /// The message nonce.
    pub nonce: u64,
}

impl<S: Spec, Mod: Module> Message<S, Mod> {
    fn new(
        sender_key: Rc<<S::CryptoSpec as CryptoSpec>::PrivateKey>,
        content: Mod::CallMessage,
        chain_id: u64,
        max_priority_fee_bips: PriorityFeeBips,
        max_fee: u64,
        gas_limit: Option<S::Gas>,
        nonce: u64,
    ) -> Self {
        Self {
            sender_key,
            content,
            details: TxDetails {
                chain_id,
                max_priority_fee_bips,
                max_fee,
                gas_limit,
            },
            nonce,
        }
    }

    pub fn to_tx<Encoder: EncodeCall<Mod>>(self) -> sov_modules_api::transaction::Transaction<S> {
        let message = Encoder::encode_call(self.content);
        Transaction::<S>::new_signed_tx(
            &self.sender_key,
            UnsignedTransaction::new(
                message,
                self.details.chain_id,
                self.details.max_priority_fee_bips,
                self.details.max_fee,
                self.nonce,
                self.details.gas_limit,
            ),
        )
    }
}

/// Trait used to generate messages from the DA layer to automate module testing
pub trait MessageGenerator {
    const DEFAULT_CHAIN_ID: u64 = config_value!("CHAIN_ID");

    /// Module where the messages originate from.
    type Module: Module;

    /// Module spec
    type Spec: Spec;

    fn create_messages(
        &self,
        chain_id: u64,
        max_priority_fee_bips: PriorityFeeBips,
        max_fee: u64,
        estimated_gas_usage: Option<<Self::Spec as Spec>::Gas>,
    ) -> Vec<Message<Self::Spec, Self::Module>>;

    /// Generates a list of messages originating from the module.
    fn create_default_messages(&self) -> Vec<Message<Self::Spec, Self::Module>> {
        self.create_messages(
            Self::DEFAULT_CHAIN_ID,
            TEST_DEFAULT_MAX_PRIORITY_FEE,
            TEST_DEFAULT_MAX_FEE,
            Some(<Self::Spec as Spec>::Gas::from_slice(
                &TEST_DEFAULT_GAS_LIMIT,
            )),
        )
    }

    fn create_default_messages_without_gas_usage(&self) -> Vec<Message<Self::Spec, Self::Module>> {
        self.create_messages(
            Self::DEFAULT_CHAIN_ID,
            TEST_DEFAULT_MAX_PRIORITY_FEE,
            TEST_DEFAULT_MAX_FEE,
            None,
        )
    }

    /// Creates a vector of raw transactions from the module.
    fn create_default_raw_txs<Encoder: EncodeCall<Self::Module>, Auth: Authenticator>(
        &self,
    ) -> Vec<RawTx> {
        self.create_raw_txs::<Encoder, Auth>(
            Self::DEFAULT_CHAIN_ID,
            TEST_DEFAULT_MAX_PRIORITY_FEE,
            TEST_DEFAULT_MAX_FEE,
            Some(<Self::Spec as Spec>::Gas::from_slice(
                &TEST_DEFAULT_GAS_LIMIT,
            )),
        )
    }

    fn create_default_raw_txs_without_gas_usage<
        Encoder: EncodeCall<Self::Module>,
        Auth: Authenticator,
    >(
        &self,
    ) -> Vec<RawTx> {
        self.create_raw_txs::<Encoder, Auth>(
            Self::DEFAULT_CHAIN_ID,
            TEST_DEFAULT_MAX_PRIORITY_FEE,
            TEST_DEFAULT_MAX_FEE,
            None,
        )
    }

    /// Creates a vector of raw transactions from the module.
    fn create_raw_txs<Encoder: EncodeCall<Self::Module>, Auth: Authenticator>(
        &self,
        chain_id: u64,
        max_priority_fee_bips: PriorityFeeBips,
        max_fee: u64,
        estimated_gas_usage: Option<<Self::Spec as Spec>::Gas>,
    ) -> Vec<RawTx> {
        let messages_iter = self
            .create_messages(
                chain_id,
                max_priority_fee_bips,
                max_fee,
                estimated_gas_usage,
            )
            .into_iter();
        let mut serialized_messages = Vec::default();
        for message in messages_iter {
            let tx = message.to_tx::<Encoder>();
            serialized_messages.push(Auth::encode(borsh::to_vec(&tx).unwrap()).unwrap());
        }
        serialized_messages
    }

    fn create_blobs<Encoder: EncodeCall<Self::Module>, Auth: Authenticator>(&self) -> Vec<u8> {
        let txs: Vec<RawTx> = self
            .create_default_raw_txs::<Encoder, Auth>()
            .into_iter()
            .collect();

        let batch = BlobData::new_batch(txs);

        borsh::to_vec(&batch).unwrap()
    }
}
