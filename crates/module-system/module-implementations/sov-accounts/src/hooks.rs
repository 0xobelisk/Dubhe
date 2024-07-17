use sov_modules_api::{CredentialId, Spec, StateAccessor};

use crate::{Account, Accounts};

impl<S: Spec> Accounts<S> {
    /// Resolve the sender's public key to an address. Return an error if the sender is not registered.
    pub fn resolve_sender_address(
        &self,
        maybe_default_address: &Option<S::Address>,
        credential_id: &CredentialId,
        state_checkpoint: &mut impl StateAccessor,
    ) -> Result<S::Address, anyhow::Error> {
        let maybe_address = self
            .accounts
            .get(credential_id, state_checkpoint)
            .map_err(|err| {
                anyhow::anyhow!("Sender account not found when resolving address: {err:?}")
            })?
            .map(|a| a.addr);

        match maybe_address {
            Some(address) => Ok(address),
            None => match maybe_default_address {
                Some(default_address) => {
                    let new_account = Account {
                        addr: default_address.clone(),
                    };

                    self.accounts
                        .set(credential_id, &new_account, state_checkpoint)?;

                    self.credential_ids.set(
                        default_address,
                        &vec![*credential_id],
                        state_checkpoint,
                    )?;

                    Ok(default_address.clone())
                }
                None => anyhow::bail!("No default address found for {}", credential_id),
            },
        }
    }
}
