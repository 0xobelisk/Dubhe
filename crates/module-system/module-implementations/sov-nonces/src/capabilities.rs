use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::{CredentialId, Spec, StateAccessor, TxScratchpad};

use crate::Nonces;

impl<S: Spec> Nonces<S> {
    /// Checks the provided nonce.
    pub fn check_nonce(
        &self,
        credential_id: &CredentialId,
        nonce_to_check: u64,
        state_checkpoint: &mut impl StateAccessor,
    ) -> Result<(), anyhow::Error> {
        let senders_expected_nonce = self
            .nonces
            .get(credential_id, state_checkpoint)?
            .unwrap_or_default();

        anyhow::ensure!(
            senders_expected_nonce == nonce_to_check,
            "Tx bad nonce for credential id: {credential_id}, expected: {senders_expected_nonce}, but found: {nonce_to_check}",
        );
        Ok(())
    }

    /// Marks a transaction as attempted, ensuring that future attempts at execution will fail
    pub fn mark_tx_attempted(
        &self,
        credential_id: &CredentialId,
        tx_scratchpad: &mut TxScratchpad<S>,
    ) {
        let nonce = self
            .nonces
            .get(credential_id, tx_scratchpad)
            .unwrap_infallible()
            .unwrap_or_default();

        let nonce = nonce + 1;

        self.nonces
            .set(credential_id, &nonce, tx_scratchpad)
            .unwrap_infallible();
    }
}
