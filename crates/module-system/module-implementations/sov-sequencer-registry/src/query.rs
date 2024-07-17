//! Defines rpc queries exposed by the sequencer registry module, along with the relevant types
use jsonrpsee::core::RpcResult;
use sov_modules_api::macros::rpc_gen;
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::{ApiStateAccessor, Spec};

use crate::SequencerRegistry;

/// The response type to the `getSequencerAddress` RPC method.
#[cfg_attr(
    feature = "native",
    derive(serde::Deserialize, serde::Serialize, Clone)
)]
#[derive(Debug, Eq, PartialEq)]
pub struct SequencerAddressResponse<S: Spec> {
    /// The rollup address of the requested sequencer.
    pub address: Option<S::Address>,
}

#[rpc_gen(client, server, namespace = "sequencer")]
impl<S: Spec, Da: sov_modules_api::DaSpec> SequencerRegistry<S, Da> {
    /// Returns the rollup address of the sequencer with the given DA address.
    ///
    /// The response only contains data if the sequencer is registered.
    #[rpc_method(name = "getSequencerAddress")]
    pub fn sequencer_address(
        &self,
        da_address: Da::Address,
        state: &mut ApiStateAccessor<S>,
    ) -> RpcResult<SequencerAddressResponse<S>> {
        Ok(SequencerAddressResponse {
            address: self
                .get_sequencer_address(da_address, state)
                .unwrap_infallible(),
        })
    }
}
