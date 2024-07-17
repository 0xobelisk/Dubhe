use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use derive_more::{Deref, DerefMut};
use jsonrpsee::core::client::ClientT;
use jsonrpsee::http_client::HttpClientBuilder;
use sov_cli::wallet_state::PrivateKeyAndAddress;
use sov_modules_api::{PrivateKey, PublicKey, Spec};
use sov_nonces::NoncesRpcClient;
use sov_rollup_interface::zk::CryptoSpec;

use crate::args::Args;

pub(crate) struct Account<S: Spec> {
    pub(crate) nonce: AtomicU64,
    pub(crate) address: S::Address,
    pub(crate) private_key: <S::CryptoSpec as CryptoSpec>::PrivateKey,
}

impl<S: Spec> Account<S> {
    fn new(private_key: <<S as Spec>::CryptoSpec as CryptoSpec>::PrivateKey) -> Self {
        let address: S::Address = (&private_key.pub_key()).into();
        Self {
            address,
            private_key,
            nonce: AtomicU64::default(),
        }
    }

    fn generate_random() -> Self {
        Self::new(<<S as Spec>::CryptoSpec as CryptoSpec>::PrivateKey::generate())
    }

    pub(crate) fn generate_n_random(n: u64) -> Vec<Self> {
        (0..n).map(|_| Self::generate_random()).collect()
    }

    pub(crate) async fn refresh_nonce(
        &mut self,
        client: &(impl ClientT + Send + Sync),
    ) -> anyhow::Result<()> {
        let credential_id = self
            .private_key
            .pub_key()
            .credential_id::<<<S as Spec>::CryptoSpec as CryptoSpec>::Hasher>();

        let nonce = NoncesRpcClient::<S>::get_nonce(client, credential_id)
            .await?
            .nonce;

        self.nonce = nonce.into();

        Ok(())
    }
}

/* */
/// The [`AccountPool`] is a structure holding a pool of accounts that consist of:
///
/// - Existing accounts whose private keys are known, that are defined at rollup
///   genesis and which have some amount of the gas token required for the rollup.

/// - New, randomly generated accounts, the amount of which is defined by the
///   `--new-users-count` command line argument when running the harness.
///
/// Upon startup, the harness determines the balances of the first group of accounts
/// and mints new tokens to them should said balance not reach a certain threshold.
/// Next, the second set of randomly generated accounts are each apportioned a share
/// of that gas token so that they may take part in sending transactions to the rollup.
#[derive(Clone, Deref, DerefMut)]
pub(crate) struct AccountPool<S: Spec>(Arc<AccountPoolInner<S>>);

pub(crate) struct AccountPoolInner<S: Spec> {
    /// Since [`Account`] isn't `Ord` we use a BTreeMap with a `u64` key that is
    /// incremented by one for each addition to the map. This way we can iterate
    /// through the accounts efficiently when sending transactions from this pool.
    ordered_accounts: BTreeMap<u64, Account<S>>,

    /// The `ordered_accounts` `BTreeMap` herein makes it inefficient to get an
    /// [`Account`] by its address, so we also store here said account addresses
    /// and the index at which it exists in the `BTreeMap`. This allow for O(1)
    /// account lookups via address, but at the cost of having to store the
    /// address twice.
    ordered_accounts_indices: HashMap<S::Address, u64>,
}

impl<S: Spec> AccountPool<S> {
    fn new_from_accounts(accounts: Vec<Account<S>>) -> Self {
        let mut ordered_accounts_indices = HashMap::<S::Address, u64>::new();
        let mut ordered_accounts = BTreeMap::<u64, Account<S>>::new();
        accounts
            .into_iter()
            .enumerate()
            .for_each(|(index, account)| {
                ordered_accounts_indices.insert(account.address.clone(), index as u64);
                ordered_accounts.insert(index as u64, account);
            });
        Self(Arc::new(AccountPoolInner {
            ordered_accounts,
            ordered_accounts_indices,
        }))
    }

    pub(crate) fn inc_nonce(&self, index: &u64) {
        self.ordered_accounts
            .get(index)
            .expect("could not get account at index {index}")
            .nonce
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn addresses(&self) -> impl Iterator<Item = &S::Address> + std::clone::Clone {
        self.ordered_accounts
            .values()
            .map(|account| &account.address)
    }

    fn from_keys_in_folder(dir: impl AsRef<Path>) -> anyhow::Result<Vec<Account<S>>> {
        let path = dir.as_ref();
        if !path.exists() {
            anyhow::bail!("Keys path {} does not exist", path.display());
        }
        if !path.is_dir() {
            anyhow::bail!("Keys path {} is not a folder", path.display());
        }

        let mut existing_keys = Vec::<Account<S>>::new();

        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                // TODO: Collect sub dirs
                tracing::warn!("SKIP DIR");
            } else {
                match PrivateKeyAndAddress::<S>::from_json_file(&path) {
                    Ok(PrivateKeyAndAddress {
                        private_key,
                        address,
                    }) => {
                        tracing::info!(address = %address, path = %path.display(), "Parsed key file");

                        let account = Account {
                            address,
                            private_key,
                            nonce: AtomicU64::new(0),
                        };

                        existing_keys.push(account);
                    }
                    Err(e) => {
                        tracing::warn!(error = ?e, path = %path.display(), "Failed to parse key file, ignoring...");
                    }
                }
            }
        }

        Ok(existing_keys)
    }

    pub(crate) async fn new_from_config(config: &Args) -> anyhow::Result<Self> {
        let mut accounts = Self::from_keys_in_folder(&config.private_keys_dir)?;

        if accounts.is_empty() {
            anyhow::bail!("Cannot proceed without any known key");
        }

        for account in &accounts {
            tracing::debug!(address = %account.address, "Address has been read from disk");
        }

        let client = HttpClientBuilder::default().build(&config.rpc_url)?;

        // Refreshing nonces before generating new users to avoid non needed RPC calls.
        for account in &mut accounts {
            account.refresh_nonce(&client).await?;
        }

        accounts.append(&mut Account::generate_n_random(config.new_users_count));

        let account_pool = AccountPool::new_from_accounts(accounts);

        Ok(account_pool) // TODO return tx batch(es) too so they can go to the blob submitter?
    }

    pub(crate) fn contains_address(&self, address: &S::Address) -> bool {
        self.ordered_accounts_indices.contains_key(address)
    }

    pub fn get_by_index(&self, index: &u64) -> Option<&Account<S>> {
        self.ordered_accounts.get(index)
    }

    #[allow(unused)]
    pub fn get_by_address(&self, address: &S::Address) -> Option<&Account<S>> {
        if let Some(index) = self.get_index(address) {
            self.ordered_accounts.get(index)
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.ordered_accounts.len()
    }

    pub fn get_index(&self, address: &S::Address) -> Option<&u64> {
        self.ordered_accounts_indices.get(address)
    }
}
