use sov_rollup_interface::da::DaSpec;

use crate::transaction::{AuthenticatedTransactionData, TransactionConsumption};
use crate::{Context, GasMeter, PreExecWorkingSet, Spec, TxScratchpad, WorkingSet};

/// The error type returned by the [`GasEnforcer::try_reserve_gas`] method.
pub struct TryReserveGasError<S: Spec, Meter: GasMeter<S::Gas>> {
    /// The reason why it was not possible to reserve gas.
    pub reason: anyhow::Error,
    /// The pre-execution working set that was used at the time of the error.
    pub pre_exec_working_set: PreExecWorkingSet<S, Meter>,
}

/// Enforces gas limits and penalties for transactions.
pub trait GasEnforcer<S: Spec, Da: DaSpec> {
    /// Checks that the transaction has enough gas to be processed.
    ///
    /// ## Note
    /// This method has to reserve enough gas to cover the pre-execution checks cost of the transaction.
    /// If the transaction doesn't have enough gas to cover the pre-execution checks, the method should return an error.
    ///
    /// ## Behavior
    /// This function **should** charge the transaction sender for the gas locked in the transaction because his balance
    /// may change during the transaction execution.
    ///
    /// ## Type-safety note
    /// TODO(@ross-weir) Make the gas meter type stricter so devs can't pass an unlimited meter
    /// while processing transactions in the normal case: <https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/833>
    #[allow(clippy::result_large_err)]
    fn try_reserve_gas<Meter: GasMeter<S::Gas>>(
        &self,
        tx: &AuthenticatedTransactionData<S>,
        context: &Context<S>,
        pre_exec_working_set: PreExecWorkingSet<S, Meter>,
    ) -> Result<WorkingSet<S>, TryReserveGasError<S, Meter>>;

    /// Allocates the gas consumed by the transaction to the base fee and the tip recipients.
    /// This method should not fail.
    ///
    /// ## Correctness note
    /// TODO(@theochap): The rollup developper has to make sure to pre-allocate enough gas to prevent the
    /// transaction sender from underpaying for this operation.
    fn allocate_consumed_gas(
        &self,
        tx_consumption: &TransactionConsumption<S::Gas>,
        tx_scratchpad: &mut TxScratchpad<S>,
    );

    /// Refunds any remaining gas to the payer after the transaction is processed.
    /// This method should not fail.
    ///
    /// ## Correctness note
    /// TODO(@theochap): The rollup developper has to make sure to pre-allocate enough gas to prevent the
    /// transaction sender from underpaying for this operation.
    fn refund_remaining_gas(
        &self,
        context: &Context<S>,
        tx_consumption: &TransactionConsumption<S::Gas>,
        tx_scratchpad: &mut TxScratchpad<S>,
    );
}
