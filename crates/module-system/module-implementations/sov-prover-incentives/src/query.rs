//! Defines rpc queries exposed by the module
use jsonrpsee::core::RpcResult;
use serde::{Deserialize, Serialize};
use sov_modules_api::macros::rpc_gen;
use sov_modules_api::prelude::UnwrapInfallible;
use sov_modules_api::{ApiStateAccessor, DaSpec};

use super::ProverIncentives;

/// The structure containing the response returned by the `get_bond_amount` query.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone)]
pub struct Response {
    /// The bond value stored as a `u64`.
    pub value: u64,
}

/// This will go away once the `REST-API` is implemented.
#[rpc_gen(client, server, namespace = "proverIncentives")]
impl<S: sov_modules_api::Spec, Da: DaSpec> ProverIncentives<S, Da> {
    /// Queries the state of the module and returns the bond amount of the address `address`.
    /// If the `address` is not bonded, returns a default value.
    #[rpc_method(name = "proverBondAmount")]
    pub fn bond_amount(
        &self,
        address: S::Address,
        state: &mut ApiStateAccessor<S>,
    ) -> RpcResult<Response> {
        Ok(Response {
            value: self
                .bonded_provers
                .get(&address, state)
                .unwrap_infallible()
                .unwrap_or_default(), // self.value.get(api_state_accessor),
        })
    }
}
