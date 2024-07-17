//! The demo-rollup supports `EVM` and `sov-module` authenticators.
use std::marker::PhantomData;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use sov_modules_api::capabilities::{
    Authenticator, AuthorizationData, UnregisteredAuthenticationError,
};
use sov_modules_api::runtime::capabilities::{
    AuthenticationError, AuthenticationResult, FatalError, RuntimeAuthenticator,
};
use sov_modules_api::{
    DaSpec, DispatchCall, GasMeter, PreExecWorkingSet, RawTx, Spec, UnlimitedGasMeter,
};
use sov_sequencer_registry::SequencerStakeMeter;

use crate::runtime::{Runtime, RuntimeCall};

impl<S: Spec, Da: DaSpec> RuntimeAuthenticator<S> for Runtime<S, Da> {
    type Decodable = <Self as DispatchCall>::Decodable;

    type SequencerStakeMeter = SequencerStakeMeter<S::Gas>;

    type AuthorizationData = AuthorizationData<S>;

    fn authenticate(
        &self,
        raw_tx: &RawTx,
        pre_exec_ws: &mut PreExecWorkingSet<S, Self::SequencerStakeMeter>,
    ) -> AuthenticationResult<S, Self::Decodable, Self::AuthorizationData> {
        let auth = Auth::try_from_slice(raw_tx.data.as_slice()).map_err(|e| {
            AuthenticationError::FatalError(FatalError::DeserializationFailed(e.to_string()))
        })?;

        match auth {
            Auth::Mod(tx) => ModAuth::<S, Da>::authenticate(&tx, pre_exec_ws),
            Auth::Evm(tx) => EvmAuth::<S, Da>::authenticate(&tx, pre_exec_ws),
        }
    }

    fn authenticate_unregistered(
        &self,
        raw_tx: &RawTx,
        pre_exec_ws: &mut PreExecWorkingSet<S, UnlimitedGasMeter<S::Gas>>,
    ) -> AuthenticationResult<
        S,
        Self::Decodable,
        Self::AuthorizationData,
        UnregisteredAuthenticationError,
    > {
        let (tx_and_raw_hash, auth_data, runtime_call) =
            sov_modules_api::capabilities::authenticate::<
                S,
                Runtime<S, Da>,
                UnlimitedGasMeter<S::Gas>,
            >(&raw_tx.data, pre_exec_ws)?;

        match &runtime_call {
            RuntimeCall::sequencer_registry(sov_sequencer_registry::CallMessage::Register {
                ..
            }) => Ok((tx_and_raw_hash, auth_data, runtime_call)),
            _ => Err(UnregisteredAuthenticationError::RuntimeCall)?,
        }
    }
}

#[derive(Debug, PartialEq, Clone, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
enum Auth {
    Evm(Vec<u8>),
    Mod(Vec<u8>),
}

/// Authenticator for the sov-module system.
pub struct ModAuth<S: Spec, Da: DaSpec> {
    _phantom: PhantomData<(S, Da)>,
}

impl<S: Spec, Da: DaSpec> Authenticator for ModAuth<S, Da> {
    type Spec = S;
    type DispatchCall = Runtime<S, Da>;
    type AuthorizationData = AuthorizationData<S>;

    fn authenticate<Meter: GasMeter<S::Gas>>(
        tx: &[u8],
        pre_exec_working_set: &mut PreExecWorkingSet<S, Meter>,
    ) -> AuthenticationResult<
        Self::Spec,
        <Self::DispatchCall as DispatchCall>::Decodable,
        Self::AuthorizationData,
    > {
        sov_modules_api::capabilities::authenticate::<Self::Spec, Self::DispatchCall, Meter>(
            tx,
            pre_exec_working_set,
        )
    }

    fn encode(tx: Vec<u8>) -> Result<RawTx, anyhow::Error> {
        let data = borsh::to_vec(&Auth::Mod(tx))?;
        Ok(RawTx { data })
    }
}

/// Authenticator for the EVM.
pub struct EvmAuth<S: Spec, Da: DaSpec> {
    _phantom: PhantomData<(S, Da)>,
}

impl<S: Spec, Da: DaSpec> Authenticator for EvmAuth<S, Da> {
    type Spec = S;
    type DispatchCall = Runtime<S, Da>;
    type AuthorizationData = AuthorizationData<S>;

    fn authenticate<Meter: GasMeter<S::Gas>>(
        tx: &[u8],
        stake_meter: &mut PreExecWorkingSet<S, Meter>,
    ) -> AuthenticationResult<
        Self::Spec,
        <Self::DispatchCall as DispatchCall>::Decodable,
        Self::AuthorizationData,
    > {
        let (tx_and_raw_hash, auth_data, runtime_call) =
            sov_evm::authenticate::<Self::Spec, Meter>(tx, stake_meter)?;
        let call = RuntimeCall::evm(runtime_call);

        Ok((tx_and_raw_hash, auth_data, call))
    }

    fn encode(tx: Vec<u8>) -> Result<RawTx, anyhow::Error> {
        let data = borsh::to_vec(&Auth::Evm(tx))?;
        Ok(RawTx { data })
    }
}
