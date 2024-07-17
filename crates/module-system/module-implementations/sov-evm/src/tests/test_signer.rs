use ethers_core::rand::rngs::StdRng;
use ethers_core::rand::SeedableRng;
use reth_primitives::{Address, Bytes as RethBytes, TransactionKind, U256, U64};
use reth_rpc_types::transaction::EIP1559TransactionRequest;
use reth_rpc_types::TypedTransactionRequest;
use secp256k1::{PublicKey, SecretKey};
use sov_eth_dev_signer::{DevSigner, SignError};
use sov_modules_api::macros::config_value;

use crate::evm::RlpEvmTransaction;

/// ETH transactions signer used in tests.
pub(crate) struct TestSigner {
    signer: DevSigner,
    address: Address,
}

impl TestSigner {
    /// Creates a new signer.
    pub(crate) fn new(secret_key: SecretKey) -> Self {
        let public_key = PublicKey::from_secret_key(secp256k1::SECP256K1, &secret_key);
        let address = reth_primitives::public_key_to_address(public_key);
        Self {
            signer: DevSigner::new(vec![secret_key]),
            address,
        }
    }

    /// Creates a new signer with a random private key.
    pub(crate) fn new_random() -> Self {
        let mut rng = StdRng::seed_from_u64(22);
        let secret_key = SecretKey::new(&mut rng);
        Self::new(secret_key)
    }

    /// Address of the transaction signer.
    pub(crate) fn address(&self) -> Address {
        self.address
    }

    /// Signs default Eip1559 transaction with to, data chain-id, and nonce overridden.
    pub(crate) fn sign_default_transaction(
        &self,
        kind: TransactionKind,
        data: Vec<u8>,
        nonce: u64,
    ) -> Result<(RlpEvmTransaction, Address), SignError> {
        let reth_tx = EIP1559TransactionRequest {
            chain_id: config_value!("CHAIN_ID"),
            nonce: U64::from(nonce),
            max_priority_fee_per_gas: Default::default(),
            max_fee_per_gas: U256::from(reth_primitives::constants::MIN_PROTOCOL_BASE_FEE * 2),
            gas_limit: U256::from(1_000_000u64),
            kind: match kind {
                TransactionKind::Create => reth_rpc_types::TransactionKind::Create,
                TransactionKind::Call(addr) => reth_rpc_types::TransactionKind::Call(addr),
            },
            value: Default::default(),
            input: RethBytes::from(data),
            access_list: Default::default(),
        };

        let reth_tx = TypedTransactionRequest::EIP1559(reth_tx);
        let signed = self.signer.sign_transaction(reth_tx, self.address)?;

        Ok((
            RlpEvmTransaction {
                rlp: signed.envelope_encoded().to_vec(),
            },
            self.address,
        ))
    }
}
