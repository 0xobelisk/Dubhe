use anyhow::{bail, Result};
use serde_with::{serde_as, DisplayFromStr};
use sov_modules_api::prelude::*;
use sov_modules_api::{CredentialId, GenesisState};

use crate::{Account, Accounts};

/// Account data for the genesis.
#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "native", derive(schemars::JsonSchema))]
pub struct AccountData<Address> {
    /// Credential ID of the account.
    #[serde_as(as = "DisplayFromStr")]
    pub credential_id: CredentialId,
    /// Address of the account.
    pub address: Address,
}

/// Initial configuration for sov-accounts module.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(
    feature = "native",
    derive(schemars::JsonSchema),
    schemars(bound = "S: ::sov_modules_api::Spec", rename = "AccountConfig")
)]
pub struct AccountConfig<S: Spec> {
    /// Accounts to initialize the rollup.
    pub accounts: Vec<AccountData<S::Address>>,
}

impl<S: Spec> Accounts<S> {
    pub(crate) fn init_module(
        &self,
        config: &<Self as sov_modules_api::Module>::Config,
        state: &mut impl GenesisState<S>,
    ) -> Result<()> {
        for acc in config.accounts.iter() {
            if self.accounts.get(&acc.credential_id, state)?.is_some() {
                bail!("Account already exists")
            }

            let new_account = Account {
                addr: acc.address.clone(),
            };

            self.accounts.set(&acc.credential_id, &new_account, state)?;

            self.credential_ids
                .set(&acc.address, &vec![acc.credential_id], state)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use sov_modules_api::PublicKey;
    use sov_test_utils::{TestHasher, TestPublicKey, TestSpec};

    use super::*;

    #[test]
    fn test_config_serialization() {
        let pub_key = &TestPublicKey::from_str(
            "1cd4e2d9d5943e6f3d12589d31feee6bb6c11e7b8cd996a393623e207da72cbf",
        )
        .unwrap();

        let config = AccountConfig::<TestSpec> {
            accounts: vec![AccountData {
                credential_id: pub_key.credential_id::<TestHasher>(),
                address: pub_key.into(),
            }],
        };

        let data = r#"
        {
            "accounts":[{"credential_id":"0xa7f38e6a301da8763eb3ba323e761c76e5122f443604c40cd0c3b74ce5a8495a","address":"sov15lecu63srk58v04nhgeruasuwmj3yt6yxczvgrxscwm5eedgf9dq5w2een"}]
        }"#;

        let parsed_config: AccountConfig<TestSpec> = serde_json::from_str(data).unwrap();
        assert_eq!(parsed_config, config);
    }
}
