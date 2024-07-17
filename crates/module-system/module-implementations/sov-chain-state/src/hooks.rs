use sov_modules_api::da::BlockHeaderTrait;
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::{Gas, KernelWorkingSet, Spec};
use sov_state::Storage;

use crate::{BlockGasInfo, ChainState, StateTransition, TransitionInProgress};

impl<S: Spec, Da: sov_modules_api::DaSpec> ChainState<S, Da> {
    /// Update the chain state at the beginning of the slot. Compute the next gas price
    pub fn begin_slot_hook(
        &self,
        slot_header: &Da::BlockHeader,
        validity_condition: &Da::ValidityCondition,
        pre_state_root: &<S::Storage as Storage>::Root,
        state: &mut KernelWorkingSet<S>,
    ) -> <S::Gas as Gas>::Price {
        let gas_info = if self
            .genesis_root
            .get(state.inner)
            .unwrap_infallible()
            .is_none()
        {
            // The genesis hash is not set, hence this is the
            // first transition right after the genesis block
            self.genesis_root
                .set(pre_state_root, state.inner)
                .unwrap_infallible();

            BlockGasInfo::new(Self::initial_gas_limit(), Self::initial_base_fee_per_gas())
        } else {
            let transition: StateTransition<S, Da> = {
                let TransitionInProgress {
                    slot_hash,
                    validity_condition,
                    gas_info,
                } = self
                    .in_progress_transition
                    .get_current(state)
                    .unwrap_infallible()
                    .expect("There should always be a transition in progress");

                StateTransition {
                    slot_hash,
                    post_state_root: pre_state_root.clone(),
                    validity_condition,
                    gas_info,
                }
            };

            let slot_number = self.true_slot_number(state).unwrap_infallible();
            self.historical_transitions
                .set(&slot_number, &transition, state.inner)
                .unwrap_infallible();

            // The base fee per gas is updated according to the EIP-1559 specification
            let computed_base_fee = Self::compute_base_fee_per_gas(&transition.gas_info);

            BlockGasInfo::new(
                // TODO(@theochap): the gas limit should be updated dynamically `<https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/271`
                Self::initial_gas_limit(),
                computed_base_fee,
            )
        };

        // Since we increment the true slot number, we have to update the working set.
        self.increment_true_slot_number(state);

        self.time.set_true_current(&slot_header.time(), state);

        let new_base_fee = gas_info.base_fee_per_gas.clone();

        self.in_progress_transition.set_true_current(
            &TransitionInProgress {
                slot_hash: slot_header.hash(),
                validity_condition: *validity_condition,
                gas_info,
            },
            state,
        );

        new_base_fee
    }

    /// Updates the gas used by the transition in progress at the end of each slot
    pub fn end_slot_hook(&self, gas_used: &S::Gas, state: &mut KernelWorkingSet<S>) {
        let mut in_progress_transition = self
            .in_progress_transition
            .get_current(state)
            .unwrap_infallible()
            .expect("There should always be a transition in progress");

        in_progress_transition
            .gas_info
            .update_gas_used(gas_used.clone());

        self.in_progress_transition
            .set_true_current(&in_progress_transition, state);
    }
}
