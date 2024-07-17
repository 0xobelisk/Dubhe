use sov_modules_api::capabilities::TryReserveGasError;
use sov_modules_api::transaction::{AuthenticatedTransactionData, TransactionConsumption};
use sov_modules_api::{
    AuthorizeTransactionError, Gas, GasMeter, PreExecWorkingSet, Spec, StateAccessorError,
    TxScratchpad, WorkingSet,
};
use thiserror::Error;

use crate::utils::IntoPayable;
use crate::{Bank, Coins, Payable, GAS_TOKEN_ID};

/// Error types that can be raised by the `reserve_gas` method
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ReserveGasErrorReason<S: Spec> {
    #[error("The payer {account} does not have an account in the `Bank` module for the gas token")]
    /// The payer does not have an account in the `Bank` module for the gas token
    AccountDoesNotExist {
        /// String representation of rollup address
        account: String,
    },
    #[error("Insufficient balance to pay for the transaction gas")]
    /// The sender balance is not high enough to pay for the gas.
    InsufficientBalanceToReserveGas,
    #[error("The current gas price is too high to cover the maximum fee for the transaction")]
    /// The current gas price is too high to cover the maximum fee for the transaction.
    CurrentGasPriceTooHigh,
    #[error("Insufficient gas locked to pay for pre execution checks. Error: {0}")]
    /// Insufficient gas locked in the transaction to cover pre-execution checks such as signature checks or transaction
    /// deserialization
    InsufficientGasForPreExecutionChecks(String),
    /// Impossible to transfer the gas from the payer to the bank
    #[error("Impossible to transfer the gas from the payer to the bank")]
    ImpossibleToTransferGas(String),
    /// Error occurred while accessing the state
    #[error("An error occurred while accessing the state: {0}")]
    StateAccessError(StateAccessorError<S::Gas>),
}

/// Error type that can be raised by the `reserve_gas` method
pub struct ReserveGasError<S: Spec, PreExecChecksMeter: GasMeter<S::Gas>> {
    /// The reason for the error.
    pub reason: ReserveGasErrorReason<S>,
    /// The pre execution working set at the time of the error.
    pub pre_exec_working_set: PreExecWorkingSet<S, PreExecChecksMeter>,
}

impl<S: Spec, Meter: GasMeter<S::Gas>> From<ReserveGasError<S, Meter>>
    for TryReserveGasError<S, Meter>
{
    fn from(value: ReserveGasError<S, Meter>) -> Self {
        Self {
            reason: value.reason.into(),
            pre_exec_working_set: value.pre_exec_working_set,
        }
    }
}

/// The [`Bank::reserve_gas`] and [`Bank::refund_remaining_gas`] are used to reserve and then lock transaction base gas and tip
impl<S: Spec> Bank<S> {
    /// Reserve the gas necessary to execute a transaction. The gas is locked at the bank's address
    /// This method loosely follows the-EIP 1559 gas price calculation.
    #[allow(clippy::result_large_err)]
    pub fn reserve_gas<PreExecChecksMeter: GasMeter<S::Gas>>(
        &self,
        tx: &AuthenticatedTransactionData<S>,
        payer: &S::Address,
        mut pre_exec_working_set: PreExecWorkingSet<S, PreExecChecksMeter>,
    ) -> Result<WorkingSet<S>, ReserveGasError<S, PreExecChecksMeter>> {
        // We need to do the explicit check (outside of a closure) because otherwise `state_checkpoint` would be captured.
        let balance =
            match self.get_balance_of(&payer.clone(), GAS_TOKEN_ID, &mut pre_exec_working_set) {
                Ok(Some(balance)) => balance,
                Ok(None) => {
                    return Err(ReserveGasError::<S, PreExecChecksMeter> {
                        pre_exec_working_set,
                        reason: ReserveGasErrorReason::AccountDoesNotExist {
                            account: payer.to_string(),
                        },
                    })
                }
                Err(err) => {
                    return Err(ReserveGasError {
                        reason: ReserveGasErrorReason::StateAccessError(err),
                        pre_exec_working_set,
                    })
                }
            };

        // the signer must be able to afford the transaction
        if balance < tx.max_fee {
            return Err(ReserveGasError::<S, PreExecChecksMeter> {
                pre_exec_working_set,
                reason: ReserveGasErrorReason::InsufficientBalanceToReserveGas,
            });
        }

        if tx.max_fee == 0 {
            tracing::warn!(
                %payer,
                "Trying to reserve gas for tx with zero max fee"
            );
        }

        // We lock the `max_fee` amount into the `Bank` module.
        // We actually **need** to do that transfer because the payer account balance may change during the execution of the transaction.
        if let Err(err) = self.transfer_from(
            payer,
            self.id.to_payable(),
            Coins {
                amount: tx.max_fee,
                token_id: GAS_TOKEN_ID,
            },
            &mut pre_exec_working_set,
        ) {
            return Err(ReserveGasError {
                reason: ReserveGasErrorReason::ImpossibleToTransferGas(err.to_string()),
                pre_exec_working_set,
            });
        }

        if let Some(gas_limit) = &tx.gas_limit {
            // We need to check the gas price in case the user has provided a gas limit.
            if tx.max_fee < gas_limit.value(pre_exec_working_set.gas_price()) {
                return Err(ReserveGasError::<S, PreExecChecksMeter> {
                    pre_exec_working_set,
                    reason: ReserveGasErrorReason::CurrentGasPriceTooHigh,
                });
            }
        }

        pre_exec_working_set
            .transfer_gas_to_working_set(tx)
            // TODO: impl From<AuthorizeTransactionError> for ReserveGasError
            .map_err(
                |AuthorizeTransactionError {
                     pre_exec_working_set,
                     reason,
                 }| ReserveGasError::<S, PreExecChecksMeter> {
                    pre_exec_working_set,
                    reason: ReserveGasErrorReason::InsufficientGasForPreExecutionChecks(
                        reason.to_string(),
                    ),
                },
            )
    }

    /// Computes and allocates the gas consumed by the transaction to the base fee and the tip recipients.
    pub fn allocate_consumed_gas(
        &self,
        // The address that receives the base fee. Typically, this is the module id of either the `ProverIncentives` or the `AttesterIncentives` module.
        base_fee_recipient: &impl Payable<S>,
        // The address that receives the transaction tip. Typically, the module id of the `SequencerRegistry` module.
        tip_recipient: &impl Payable<S>,
        tx_consumption: &TransactionConsumption<S::Gas>,
        tx_scratchpad: &mut TxScratchpad<S>,
    ) {
        self.transfer_from(
            self.id.to_payable(),
            base_fee_recipient.as_token_holder(),
            Coins {
                amount: tx_consumption.base_fee_value(),
                token_id: GAS_TOKEN_ID,
            },
            tx_scratchpad,
        )
        .expect("Transferring the consumed base fee gas is infallible");

        self.transfer_from(
            self.id.to_payable(),
            tip_recipient.as_token_holder(),
            Coins {
                amount: tx_consumption.priority_fee(),
                token_id: GAS_TOKEN_ID,
            },
            tx_scratchpad,
        )
        .expect("Transferring the consumed gas tip is infallible");
    }

    /// Refunds any remaining gas to the payer from the bank module after the transaction is processed.
    pub fn refund_remaining_gas(
        &self,
        payer: &S::Address,
        tx_consumption: &TransactionConsumption<S::Gas>,
        tx_scratchpad: &mut TxScratchpad<S>,
    ) {
        // We refund the payer. We need to give back the remaining funds on the gas meter, plus the unspent tip.
        // This is also the maximum fee minus everything that was spent for the tip and base fee (ie the total reward).
        self.transfer_from(
            self.id.to_payable(),
            payer,
            Coins {
                amount: tx_consumption.remaining_funds(),
                token_id: GAS_TOKEN_ID,
            },
            tx_scratchpad,
        )
        .expect("Refunding unspent gas is infallible");
    }
}
