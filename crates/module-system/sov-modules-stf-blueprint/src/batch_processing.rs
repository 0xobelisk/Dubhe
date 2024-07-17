#[cfg(all(target_os = "zkvm", feature = "bench"))]
use risc0_cycle_macros::cycle_tracker;
use sov_modules_api::capabilities::{
    AuthenticationError, AuthenticationResult, AuthorizeSequencerError, FatalError, GasEnforcer,
    HasCapabilities, RuntimeAuthenticator, RuntimeAuthorization, SequencerAuthorization,
    TryReserveGasError, UnregisteredAuthenticationError,
};
use sov_modules_api::runtime::capabilities::KernelSlotHooks;
use sov_modules_api::transaction::{
    forced_sequencer_registration_cost, AuthenticatedTransactionData, SequencerReward,
};
use sov_modules_api::{
    BatchWithId, Context, DaSpec, DispatchCall, Error, Gas, GasArray, GasMeter, PreExecWorkingSet,
    RawTx, Spec, StateCheckpoint, TxScratchpad, UnlimitedGasMeter, WorkingSet,
};
use sov_sequencer_registry::BatchSequencerOutcome;
use tracing::{debug, error, info, warn};

use crate::stf_blueprint::convert_to_runtime_events;
use crate::{
    ApplyTxResult, Runtime, SkippedReason, TxEffect, TxProcessingError, TxProcessingErrorReason,
    TxReceiptContents,
};

/// The receipt type for a transacition using the STF blueprint.
pub type TransactionReceipt = sov_rollup_interface::stf::TransactionReceipt<TxReceiptContents>;

type ApplyBatchResult<T> = Result<T, ApplyBatchError>;
/// The receipt for a batch using the STF blueprint.
pub type BatchReceipt =
    sov_rollup_interface::stf::BatchReceipt<BatchSequencerOutcome, TxReceiptContents>;

#[allow(type_alias_bounds)]
pub(crate) type ApplyBatch = ApplyBatchResult<BatchReceipt>;

pub(crate) enum ApplyBatchError {
    // Contains batch hash
    Ignored {
        /// The hash of the batch
        hash: [u8; 32],
        /// The reason the batch was ignored
        reason: String,
    },
    Slashed {
        // Contains batch hash
        hash: [u8; 32],
        tx_receipts: Vec<TransactionReceipt>,
        // TODO(@theochap) `<https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/595>`: change to `S::Gas`
        gas_price: Vec<u64>,
        reason: FatalError,
    },
}

impl From<ApplyBatchError> for BatchReceipt {
    fn from(value: ApplyBatchError) -> Self {
        match value {
            ApplyBatchError::Ignored { hash, reason } => BatchReceipt {
                batch_hash: hash,
                tx_receipts: Vec::new(),
                inner: BatchSequencerOutcome::Ignored(reason),
                gas_price: Vec::new(),
            },
            ApplyBatchError::Slashed {
                hash,
                tx_receipts,
                gas_price,
                reason,
            } => BatchReceipt {
                batch_hash: hash,
                tx_receipts,
                inner: BatchSequencerOutcome::Slashed(reason),
                gas_price,
            },
        }
    }
}

#[tracing::instrument(skip_all, name = "StfBlueprint::apply_batch")]
#[cfg_attr(all(target_os = "zkvm", feature = "bench"), cycle_tracker)]
pub(crate) fn apply_batch<S, Da, RT, K>(
    runtime: &RT,
    mut checkpoint: StateCheckpoint<S>,
    batch_with_id: BatchWithId,
    sequencer_da_address: &Da::Address,
    gas_price: &<S::Gas as Gas>::Price,
    height: u64,
    is_registered_sequencer: bool,
) -> (ApplyBatch, StateCheckpoint<S>, S::Gas)
where
    S: Spec,
    Da: DaSpec,
    RT: Runtime<S, Da>,
    K: KernelSlotHooks<S, Da>,
{
    debug!(
        batch_id = hex::encode(batch_with_id.id),
        sequencer_da_address = %sequencer_da_address,
        ?gas_price,
        "Applying a batch"
    );

    // ApplyBlobHook: begin
    if let Err(e) = runtime.begin_batch_hook(&batch_with_id, sequencer_da_address, &mut checkpoint)
    {
        error!(
            error = %e,
            batch_id = hex::encode(batch_with_id.id),
            "Error: The batch was rejected by the 'begin_batch_hook' hook. Skipping batch without slashing the sequencer",
        );

        return (
                Err(ApplyBatchError::Ignored {
                    hash: batch_with_id.id,
                    reason: "Error: The batch was rejected by the 'begin_batch_hook' hook. Skipping batch without slashing the sequencer".to_string(),
                }),
                checkpoint,
                S::Gas::zero(),
            );
    }

    let raw_txs = batch_with_id.batch.txs;

    let mut tx_receipts = Vec::with_capacity(raw_txs.len());
    let mut gas_used = S::Gas::zero();
    let mut accumulated_reward = SequencerReward::ZERO;

    debug!(
        batch_id = hex::encode(batch_with_id.id),
        txs_num = raw_txs.len(),
        "Verifying & executing transactions"
    );

    for (idx, raw_tx) in raw_txs.iter().enumerate() {
        let tx_scratchpad = checkpoint.to_tx_scratchpad();
        let process_tx_result = if is_registered_sequencer {
            process_tx(
                runtime,
                raw_tx,
                sequencer_da_address,
                gas_price,
                height,
                tx_scratchpad,
            )
        } else {
            process_unauthorized_tx(
                runtime,
                raw_tx,
                sequencer_da_address,
                gas_price,
                height,
                tx_scratchpad,
            )
        };

        match process_tx_result {
            Err(TxProcessingError {
                tx_scratchpad,
                reason,
            }) => {
                checkpoint = tx_scratchpad.commit();
                match reason {
                    TxProcessingErrorReason::SequencerUnauthorized(reason) => {
                        let remaining = raw_txs.len() - idx - 1;
                        error!(
                            reason = %reason,
                            sequencer_da_address = %sequencer_da_address,
                            tx_idx = %idx,
                            remaining = remaining,
                            "The transaction was rejected by the 'authorize_sequencer' capability. Dropping the remaining transactions in that batch",
                        );
                        break;
                    }

                    // If the sequencer raised a fatal error then he needs to get slashed and we stop applying the batch
                    TxProcessingErrorReason::AuthenticationError(
                        AuthenticationError::FatalError(err),
                    ) => {
                        error!(
                                sequencer_da_address = %sequencer_da_address,
                                err=%err, "Tx authentication raised a fatal error, sequencer slashed");

                        runtime.end_batch_hook(
                            BatchSequencerOutcome::Slashed(err.clone()),
                            sequencer_da_address,
                            &mut checkpoint,
                        );

                        return (
                            Err(ApplyBatchError::Slashed {
                                hash: batch_with_id.id,
                                reason: err,
                                tx_receipts,
                                gas_price: gas_price.to_vec(),
                            }),
                            checkpoint,
                            gas_used,
                        );
                    }
                    TxProcessingErrorReason::InvalidUnregisteredTx(reason) => {
                        warn!(
                            sequencer_da_address = %sequencer_da_address,
                            reason = %reason,
                            "Processing of unregistered sequencer transaction raised error, skipping"
                        );
                        runtime.end_batch_hook(
                            BatchSequencerOutcome::Ignored(reason.clone()),
                            sequencer_da_address,
                            &mut checkpoint,
                        );
                        return (
                            Err(ApplyBatchError::Ignored {
                                hash: batch_with_id.id,
                                reason,
                            }),
                            checkpoint,
                            gas_used,
                        );
                    }

                    // In these cases the sequencer is penalized and we can just ignore the outcome
                    err => {
                        match TryInto::<(SkippedReason, [u8; 32])>::try_into(err) {
                            Ok((reason, raw_tx_hash)) => {
                                warn!(
                                    error = %reason,
                                    raw_tx_hash = hex::encode(raw_tx_hash),
                                    tx_idx = %idx,
                                    "An error occurred while processing a transaction. The transaction was not executed. The sequencer was penalized.",
                                );

                                let tx_receipt = TransactionReceipt {
                                    tx_hash: raw_tx_hash,
                                    body_to_save: None,
                                    events: Vec::new(),
                                    receipt: TxEffect::Skipped(reason),
                                    gas_used: S::Gas::zero().to_vec(),
                                };

                                tx_receipts.push(tx_receipt);
                            }
                            Err(err) => {
                                // TODO: https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/901
                                error!(error = ?err, "Transaction will be completely forgotten, just like tears in the rain.");
                            }
                        }
                    }
                }
            }
            Ok(ApplyTxResult {
                tx_scratchpad,
                receipt,
                sequencer_reward,
            }) => {
                checkpoint = tx_scratchpad.commit();

                gas_used.combine(&S::Gas::from_slice(&receipt.gas_used));
                tx_receipts.push(receipt);

                accumulated_reward.accumulate(sequencer_reward);
            }
        }
    }

    let sequencer_outcome = if is_registered_sequencer {
        BatchSequencerOutcome::Rewarded(accumulated_reward)
    } else {
        BatchSequencerOutcome::NotRewardable
    };

    runtime.end_batch_hook(
        sequencer_outcome.clone(),
        sequencer_da_address,
        &mut checkpoint,
    );

    (
        Ok(BatchReceipt {
            batch_hash: batch_with_id.id,
            tx_receipts,
            inner: sequencer_outcome,
            gas_price: gas_price.to_vec(),
        }),
        checkpoint,
        gas_used,
    )
}

/// Executes the entire transaction lifecycle.
#[allow(clippy::result_large_err)]
pub fn process_tx<S: Spec, D: DaSpec, R: Runtime<S, D>>(
    runtime: &R,
    raw_tx: &RawTx,
    // TODO <`https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/728`>: group constant variables in the stf-blueprint
    sequencer_da_address: &D::Address,
    gas_price: &<S::Gas as Gas>::Price,
    height: u64,
    scratchpad: TxScratchpad<S>,
) -> Result<ApplyTxResult<S>, TxProcessingError<S>> {
    // Checks the sequencer balance before the transaction is executed.
    // If the sequencer balance is not high enough, the transaction is rejected.
    let mut pre_exec_working_set = match runtime.capabilities().authorize_sequencer(
        sequencer_da_address,
        gas_price,
        scratchpad,
    ) {
        Ok(pre_exec_working_set) => pre_exec_working_set,
        Err(AuthorizeSequencerError {
            reason,
            tx_scratchpad,
        }) => {
            return Err(TxProcessingError {
                tx_scratchpad,
                reason: TxProcessingErrorReason::SequencerUnauthorized(reason.to_string()),
            });
        }
    };

    let (tx, auth_data, message) =
        match authenticate_with_cycle_count(runtime, raw_tx, &mut pre_exec_working_set) {
            Err(AuthenticationError::FatalError(reason)) => {
                return Err(TxProcessingError {
                    tx_scratchpad: pre_exec_working_set.into(),
                    reason: TxProcessingErrorReason::AuthenticationError(
                        AuthenticationError::FatalError(reason),
                    ),
                });
            }
            Err(AuthenticationError::Invalid(reason)) => {
                // Applies the outcome of the transaction execution to update the sequencer's state.
                let tx_scratchpad = runtime.capabilities().penalize_sequencer(
                    sequencer_da_address,
                    AuthenticationError::Invalid(reason.clone()),
                    pre_exec_working_set,
                );

                return Err(TxProcessingError {
                    tx_scratchpad,
                    reason: TxProcessingErrorReason::AuthenticationError(
                        AuthenticationError::Invalid(reason),
                    ),
                });
            }
            Ok((tx, auth_data, message)) => (tx, auth_data, message),
        };

    let raw_tx_hash = &tx.raw_tx_hash;
    let tx = &tx.authenticated_tx;

    let maybe_ctx = runtime.capabilities().resolve_context(
        &auth_data,
        sequencer_da_address,
        height,
        &mut pre_exec_working_set,
    );
    let ctx = match maybe_ctx {
        Ok(ctx) => ctx,
        Err(err) => {
            let err_string = err.to_string();
            // We penalize the sequencer for the fixed amount of gas that was used to execute the transaction.
            let tx_scratchpad = runtime.capabilities().penalize_sequencer(
                sequencer_da_address,
                err,
                pre_exec_working_set,
            );

            return Err(TxProcessingError {
                tx_scratchpad,
                reason: TxProcessingErrorReason::CannotResolveContext {
                    reason: err_string,
                    raw_tx_hash: *raw_tx_hash,
                },
            });
        }
    };

    // Check that the transaction isn't a duplicate
    if let Err(err) =
        runtime
            .capabilities()
            .check_uniqueness(&auth_data, &ctx, &mut pre_exec_working_set)
    {
        let err_string = err.to_string();

        // We penalize the sequencer for the fixed amount of gas that was used to execute the transaction.
        let tx_scratchpad = runtime.capabilities().penalize_sequencer(
            sequencer_da_address,
            err,
            pre_exec_working_set,
        );

        return Err(TxProcessingError {
            tx_scratchpad,
            reason: TxProcessingErrorReason::Nonce {
                reason: err_string,
                raw_tx_hash: *raw_tx_hash,
            },
        });
    }

    let working_set = match runtime
        .capabilities()
        .try_reserve_gas(tx, &ctx, pre_exec_working_set)
    {
        Ok(working_set) => working_set,
        Err(TryReserveGasError {
            reason,
            pre_exec_working_set,
        }) => {
            let reason_string = reason.to_string();
            // We penalize the sequencer for the fixed amount of gas that was used to execute the transaction.
            let tx_scratchpad = runtime.capabilities().penalize_sequencer(
                sequencer_da_address,
                reason,
                pre_exec_working_set,
            );

            return Err(TxProcessingError {
                tx_scratchpad,
                reason: TxProcessingErrorReason::CannotReserveGas {
                    reason: reason_string,
                    raw_tx_hash: *raw_tx_hash,
                },
            });
        }
    };

    // If the transaction is valid, execute it and apply the changes to the state.
    Ok(apply_tx(
        runtime,
        ctx,
        tx,
        &auth_data,
        raw_tx_hash,
        message,
        working_set,
        sequencer_da_address,
    ))
}

#[cfg_attr(all(target_os = "zkvm", feature = "bench"), cycle_tracker)]
fn authenticate_with_cycle_count<S: Spec, Da: DaSpec, R: Runtime<S, Da>>(
    runtime: &R,
    raw_tx: &RawTx,
    pre_exec_working_set: &mut PreExecWorkingSet<
        S,
        <R as HasCapabilities<S, Da>>::SequencerStakeMeter,
    >,
) -> AuthenticationResult<
    S,
    <R as RuntimeAuthenticator<S>>::Decodable,
    <R as RuntimeAuthenticator<S>>::AuthorizationData,
> {
    runtime.authenticate(raw_tx, pre_exec_working_set)
}

#[allow(clippy::result_large_err)]
pub fn process_unauthorized_tx<S: Spec, D: DaSpec, R: Runtime<S, D>>(
    runtime: &R,
    raw_tx: &RawTx,
    sequencer_da_address: &D::Address,
    gas_price: &<S::Gas as Gas>::Price,
    height: u64,
    tx_scratchpad: TxScratchpad<S>,
) -> Result<ApplyTxResult<S>, TxProcessingError<S>> {
    let mut pre_exec_working_set =
        tx_scratchpad.to_pre_exec_working_set(UnlimitedGasMeter::new_with_price(gas_price.clone()));

    let (tx, auth_data, message) = match authenticate_unregistered_with_cycle_count(
        runtime,
        raw_tx,
        &mut pre_exec_working_set,
    ) {
        Ok(v) => v,
        Err(e) => {
            return Err(TxProcessingError {
                reason: TxProcessingErrorReason::InvalidUnregisteredTx(e.to_string()),
                tx_scratchpad: pre_exec_working_set.into(),
            });
        }
    };

    let raw_tx_hash = &tx.raw_tx_hash;
    let tx = &tx.authenticated_tx;

    let ctx = match runtime.capabilities().resolve_unregistered_context(
        &auth_data,
        height,
        &mut pre_exec_working_set,
    ) {
        Ok(ctx) => ctx,
        Err(e) => {
            return Err(TxProcessingError {
                tx_scratchpad: pre_exec_working_set.into(),
                reason: TxProcessingErrorReason::CannotResolveContext {
                    reason: e.to_string(),
                    raw_tx_hash: *raw_tx_hash,
                },
            });
        }
    };

    // Check that the transaction isn't a duplicate
    if let Err(e) =
        runtime
            .capabilities()
            .check_uniqueness(&auth_data, &ctx, &mut pre_exec_working_set)
    {
        return Err(TxProcessingError {
            tx_scratchpad: pre_exec_working_set.into(),
            reason: TxProcessingErrorReason::Nonce {
                reason: e.to_string(),
                raw_tx_hash: *raw_tx_hash,
            },
        });
    }

    if let Err(e) = pre_exec_working_set.charge_gas(&forced_sequencer_registration_cost::<S>()) {
        return Err(TxProcessingError {
            tx_scratchpad: pre_exec_working_set.into(),
            reason: TxProcessingErrorReason::CannotReserveGas {
                reason: e.to_string(),
                raw_tx_hash: *raw_tx_hash,
            },
        });
    }

    let working_set = match runtime
        .capabilities()
        .try_reserve_gas(tx, &ctx, pre_exec_working_set)
    {
        Ok(working_set) => working_set,
        Err(TryReserveGasError {
            reason,
            pre_exec_working_set,
        }) => {
            return Err(TxProcessingError {
                tx_scratchpad: pre_exec_working_set.into(),
                reason: TxProcessingErrorReason::CannotReserveGas {
                    reason: reason.to_string(),
                    raw_tx_hash: *raw_tx_hash,
                },
            });
        }
    };

    // If the transaction is valid, execute it and apply the changes to the state.
    Ok(apply_tx(
        runtime,
        ctx,
        tx,
        &auth_data,
        raw_tx_hash,
        message,
        working_set,
        sequencer_da_address,
    ))
}

#[cfg_attr(all(target_os = "zkvm", feature = "bench"), cycle_tracker)]
fn authenticate_unregistered_with_cycle_count<S: Spec, Da: DaSpec, R: Runtime<S, Da>>(
    runtime: &R,
    raw_tx: &RawTx,
    pre_exec_working_set: &mut PreExecWorkingSet<S, UnlimitedGasMeter<S::Gas>>,
) -> AuthenticationResult<
    S,
    <R as RuntimeAuthenticator<S>>::Decodable,
    <R as RuntimeAuthenticator<S>>::AuthorizationData,
    UnregisteredAuthenticationError,
> {
    runtime.authenticate_unregistered(raw_tx, pre_exec_working_set)
}

/// Applies a single transaction to the current state. In normal execution, we commit twice times execution:
/// 1. After the pre-dispatch hook. This ensures that the gas charges are paid even if the transaction fails later during execution
/// 2. After the post-dispatch hook. This ensures that the transaction can be reverted by the post-dispatch hook if desired.
#[cfg_attr(all(target_os = "zkvm", feature = "bench"), cycle_tracker)]
#[allow(clippy::too_many_arguments)]
fn apply_tx<S, RT, Da>(
    runtime: &RT,
    ctx: Context<S>,
    tx: &AuthenticatedTransactionData<S>,
    auth_data: &<RT as RuntimeAuthenticator<S>>::AuthorizationData,
    raw_tx_hash: &[u8; 32],
    message: <RT as DispatchCall>::Decodable,
    mut working_set: WorkingSet<S>,
    sequencer: &Da::Address,
) -> ApplyTxResult<S>
where
    S: Spec,
    Da: DaSpec,
    RT: Runtime<S, Da>,
{
    let tx_result = attempt_tx(tx, message, &ctx, runtime, &mut working_set);
    let (mut tx_scratchpad, receipt, transaction_consumption) = match tx_result {
        Ok(_) => {
            let (tx_scratchpad, transaction_consumption, events) = working_set.finalize();

            (
                tx_scratchpad,
                TransactionReceipt {
                    tx_hash: *raw_tx_hash,
                    body_to_save: None,
                    events: convert_to_runtime_events::<S, RT, Da>(events),
                    receipt: TxEffect::Successful(()),
                    gas_used: transaction_consumption.base_fee().to_vec(),
                },
                transaction_consumption,
            )
        }
        Err(e) => {
            // It's expected that transactions will revert, so we log them at the info level.
            info!(
                error = %e,
                raw_tx_hash = hex::encode(raw_tx_hash),
                "Tx was reverted",
            );
            // the transaction causing invalid state transition is reverted,
            // but we don't slash and continue processing remaining transactions.
            // working_set.revert_in_place();
            let (tx_scratchpad, transaction_consumption) = working_set.revert();

            let receipt = TransactionReceipt {
                tx_hash: *raw_tx_hash,
                body_to_save: None,
                events: vec![], // As in Ethereum, reverted transactions don't emit events
                receipt: TxEffect::Reverted(e),
                gas_used: transaction_consumption.base_fee().to_vec(),
            };

            (tx_scratchpad, receipt, transaction_consumption)
        }
    };

    runtime
        .capabilities()
        .mark_tx_attempted(auth_data, sequencer, &mut tx_scratchpad);

    runtime
        .capabilities()
        .refund_remaining_gas(&ctx, &transaction_consumption, &mut tx_scratchpad);

    runtime
        .capabilities()
        .allocate_consumed_gas(&transaction_consumption, &mut tx_scratchpad);

    debug!(
    tx_hash = hex::encode(raw_tx_hash),
    receipt = ?receipt.receipt,
    consumption = %transaction_consumption,
    "Transaction has been executed",
    );

    ApplyTxResult::<S> {
        tx_scratchpad,
        receipt,
        sequencer_reward: transaction_consumption.into(),
    }
}

fn attempt_tx<S: Spec, Da: DaSpec, RT: Runtime<S, Da>>(
    tx: &AuthenticatedTransactionData<S>,
    message: <RT as DispatchCall>::Decodable,
    ctx: &Context<S>,
    runtime: &RT,
    state: &mut WorkingSet<S>,
) -> Result<(), Error> {
    runtime.pre_dispatch_tx_hook(tx, state)?;

    runtime.dispatch_call(message, state, ctx)?;

    runtime.post_dispatch_tx_hook(tx, ctx, state)?;

    Ok(())
}
