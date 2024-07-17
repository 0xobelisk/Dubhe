use anyhow::{anyhow, Result};
use sov_modules_api::{CallResponse, Context, CredentialId, Spec, StateReader, TxState};
use sov_state::namespaces::User;

use crate::{Account, Accounts};

/// Represents the available call messages for interacting with the sov-accounts module.
#[cfg_attr(
    feature = "native",
    derive(schemars::JsonSchema),
    derive(sov_modules_api::macros::CliWalletArg)
)]
#[derive(
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    PartialEq,
    Clone,
)]
pub enum CallMessage {
    /// Inserts a new credential id for the corresponding Account.
    InsertCredentialId(
        /// The new credential id.
        CredentialId,
    ),
}

impl<S: Spec> Accounts<S> {
    pub(crate) fn insert_credential_id(
        &self,
        new_credential_id: CredentialId,
        context: &Context<S>,
        state: &mut impl TxState<S>,
    ) -> Result<CallResponse> {
        self.exit_if_account_exists(&new_credential_id, state)?;

        // Insert the new credential id.
        let account = Account {
            addr: context.sender().clone(),
        };

        self.accounts.set(&new_credential_id, &account, state)?;

        let mut credential_ids = self
            .credential_ids
            .get_or_err(context.sender(), state)
            .map_err(|e| anyhow::anyhow!("Error raised while getting credential ids: {e:?}"))??;

        credential_ids.push(new_credential_id);
        self.credential_ids
            .set(context.sender(), &credential_ids, state)?;

        Ok(CallResponse::default())
    }

    fn exit_if_account_exists(
        &self,
        new_credential_id: &CredentialId,
        state: &mut impl StateReader<User>,
    ) -> Result<()> {
        anyhow::ensure!(
            self.accounts
                .get(new_credential_id, state)
                .map_err(|err| anyhow!("Error raised while getting account: {err:?}"))?
                .is_none(),
            "New CredentialId already exists"
        );
        Ok(())
    }
}
