use anyhow::{bail, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sov_modules_api::GenesisState;

use crate::token::Token;
use crate::utils::TokenHolderRef;
use crate::{Bank, TokenId, GAS_TOKEN_ID};

/// Initial configuration for sov-bank module.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(
    feature = "native",
    derive(schemars::JsonSchema),
    schemars(bound = "S: ::sov_modules_api::Spec", rename = "BankConfig")
)]
#[serde(bound = "S::Address: Serialize + DeserializeOwned")]
pub struct BankConfig<S: sov_modules_api::Spec> {
    /// Configuration for the gas token
    pub gas_token_config: GasTokenConfig<S>,
    /// A list of configurations for any other tokens to create at genesis
    pub tokens: Vec<TokenConfig<S>>,
}

/// [`TokenConfig`] specifies a configuration used when generating a token for the bank
/// module.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(
    feature = "native",
    derive(schemars::JsonSchema),
    schemars(bound = "S: ::sov_modules_api::Spec", rename = "TokenConfig")
)]
#[serde(bound = "S::Address: Serialize + DeserializeOwned")]
pub struct TokenConfig<S: sov_modules_api::Spec> {
    /// The name of the token.
    pub token_name: String,
    /// Predetermined ID of the token. Allowed only for genesis tokens.
    pub token_id: TokenId,
    /// A vector of tuples containing the initial addresses and balances (as u64)
    pub address_and_balances: Vec<(S::Address, u64)>,
    /// The addresses that are authorized to mint the token.
    pub authorized_minters: Vec<S::Address>,
}

impl<S: sov_modules_api::Spec> core::fmt::Display for TokenConfig<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let address_and_balances = self
            .address_and_balances
            .iter()
            .map(|(address, balance)| format!("({}, {})", address, balance))
            .collect::<Vec<String>>()
            .join(", ");

        let authorized_minters = self
            .authorized_minters
            .iter()
            .map(|minter| minter.to_string())
            .collect::<Vec<String>>()
            .join(", ");

        write!(
            f,
            "TokenConfig {{ token_name: {}, token_id: {}, address_and_balances: [{}], authorized_minters: [{}] }}",
            self.token_name,
            self.token_id,
            address_and_balances,
            authorized_minters,
        )
    }
}

/// [`GasTokenConfig`] specifies a configuration for the rollup's gas token.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(
    feature = "native",
    derive(schemars::JsonSchema),
    schemars(bound = "S: ::sov_modules_api::Spec", rename = "GasTokenConfig")
)]
#[serde(bound = "S::Address: Serialize + DeserializeOwned")]
pub struct GasTokenConfig<S: sov_modules_api::Spec> {
    /// The name of the token.
    pub token_name: String,
    /// A vector of tuples containing the initial addresses and balances (as u64)
    pub address_and_balances: Vec<(S::Address, u64)>,
    /// The addresses that are authorized to mint the token.
    pub authorized_minters: Vec<S::Address>,
}

impl<S: sov_modules_api::Spec> From<GasTokenConfig<S>> for TokenConfig<S> {
    fn from(gas_token_config: GasTokenConfig<S>) -> Self {
        TokenConfig {
            token_name: gas_token_config.token_name,
            token_id: crate::GAS_TOKEN_ID,
            address_and_balances: gas_token_config.address_and_balances,
            authorized_minters: gas_token_config.authorized_minters,
        }
    }
}

impl<S: sov_modules_api::Spec> core::fmt::Display for GasTokenConfig<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let address_and_balances = self
            .address_and_balances
            .iter()
            .map(|(address, balance)| format!("({}, {})", address, balance))
            .collect::<Vec<String>>()
            .join(", ");

        write!(
            f,
            "TokenConfig {{ token_name: {}, token_id: {}, address_and_balances: [{}] }}",
            self.token_name, GAS_TOKEN_ID, address_and_balances,
        )
    }
}

impl<S: sov_modules_api::Spec> Bank<S> {
    /// Init an instance of the bank module from the configuration `config`.
    /// For each token in the `config`, calls the [`Token::create`] function to create
    /// the token. Upon success, updates the token set if the token ID doesn't already exist.
    pub(crate) fn init_module(
        &self,
        config: &<Self as sov_modules_api::Module>::Config,
        state: &mut impl GenesisState<S>,
    ) -> Result<()> {
        let parent_prefix = self.tokens.prefix();
        let gas_token_config: TokenConfig<S> = config.gas_token_config.clone().into();
        tracing::debug!(token_id = %GAS_TOKEN_ID, token_name = %gas_token_config.token_name, "Gas token");
        for token_config in std::iter::once(&gas_token_config).chain(config.tokens.iter()) {
            let token_id = &token_config.token_id;
            tracing::debug!(
                %token_config,
                token_id = %token_id,
                "Genesis of the token");

            let authorized_minters = token_config
                .authorized_minters
                .iter()
                .map(|minter| TokenHolderRef::<'_, S>::from(&minter))
                .collect::<Vec<_>>();

            let address_and_balances = token_config
                .address_and_balances
                .iter()
                .map(|(address, balance)| (TokenHolderRef::<'_, S>::from(&address), *balance))
                .collect::<Vec<_>>();

            let token = Token::<S>::create_with_token_id(
                &token_config.token_name,
                &address_and_balances,
                &authorized_minters,
                token_id,
                parent_prefix,
                state,
            )?;

            if self.tokens.get(token_id, state)?.is_some() {
                bail!("token ID {} already exists", token_config.token_id);
            }

            self.tokens.set(token_id, &token, state)?;
            tracing::debug!(
                token_name = %token.name,
                token_id = %token_id,
                "Token has been created"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use sov_modules_api::prelude::serde_json;
    use sov_modules_api::{AddressBech32, Spec};
    use sov_test_utils::TestSpec;

    use super::*;

    #[test]
    fn test_config_serialization() {
        let sender_address: <TestSpec as Spec>::Address = AddressBech32::from_str(
            "sov1l6n2cku82yfqld30lanm2nfw43n2auc8clw7r5u5m6s7p8jrm4zqrr8r94",
        )
        .unwrap()
        .into();
        let token_id =
            TokenId::from_str("token_1rwrh8gn2py0dl4vv65twgctmlwck6esm2as9dftumcw89kqqn3nqrduss6")
                .unwrap();

        let config = BankConfig::<TestSpec> {
            gas_token_config: GasTokenConfig {
                token_name: "sov-gas-token".to_owned(),
                address_and_balances: vec![(sender_address, 100000000)],
                authorized_minters: vec![sender_address],
            },
            tokens: vec![TokenConfig {
                token_name: "sov-demo-token".to_owned(),
                token_id,
                address_and_balances: vec![(sender_address, 1000)],
                authorized_minters: vec![sender_address],
            }],
        };

        let data = r#"
        {
            "gas_token_config": {
                "token_name":"sov-gas-token",
                "address_and_balances":[["sov1l6n2cku82yfqld30lanm2nfw43n2auc8clw7r5u5m6s7p8jrm4zqrr8r94",100000000]],
                "authorized_minters":["sov1l6n2cku82yfqld30lanm2nfw43n2auc8clw7r5u5m6s7p8jrm4zqrr8r94"]
            },
            "tokens":[
                {
                    "token_name":"sov-demo-token",
                    "token_id": "token_1rwrh8gn2py0dl4vv65twgctmlwck6esm2as9dftumcw89kqqn3nqrduss6",
                    "address_and_balances":[["sov1l6n2cku82yfqld30lanm2nfw43n2auc8clw7r5u5m6s7p8jrm4zqrr8r94",1000]],
                    "authorized_minters":["sov1l6n2cku82yfqld30lanm2nfw43n2auc8clw7r5u5m6s7p8jrm4zqrr8r94"]
                }
            ]
        }"#;

        let parsed_config: BankConfig<TestSpec> = serde_json::from_str(data).unwrap();

        assert_eq!(config, parsed_config);
    }
}
