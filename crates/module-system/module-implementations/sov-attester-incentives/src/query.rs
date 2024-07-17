//! Defines the query methods for the attester incentives module
use serde::{Deserialize, Serialize};
use sov_modules_api::{StateReader, WorkingSet};
use sov_state::storage::{SlotKey, Storage, StorageProof};
use sov_state::User;

use super::AttesterIncentives;
use crate::call::Role;

/// The response type to the `getBondAmount` query.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct BondAmountResponse {
    /// The value of the bond
    pub value: u64,
}

// TODO: implement rpc_gen macro
impl<S, Da> AttesterIncentives<S, Da>
where
    S: sov_modules_api::Spec,
    Da: sov_modules_api::DaSpec,
{
    /// Queries the state of the module.
    pub fn get_bond_amount<Reader: StateReader<User>>(
        &self,
        address: S::Address,
        role: Role,
        state: &mut Reader,
    ) -> Result<BondAmountResponse, Reader::Error> {
        Ok(match role {
            Role::Attester => BondAmountResponse {
                value: self
                    .bonded_attesters
                    .get(&address, state)?
                    .unwrap_or_default(),
            },
            Role::Challenger => BondAmountResponse {
                value: self
                    .bonded_challengers
                    .get(&address, state)?
                    .unwrap_or_default(),
            },
        })
    }

    /// Gives storage key for given address
    pub fn get_attester_storage_key(&self, address: S::Address) -> SlotKey {
        let prefix = self.bonded_attesters.prefix();
        let codec = self.bonded_attesters.codec();
        // Maybe we will need to store the namespace somewhere in the rollup
        SlotKey::new(prefix, &address, codec)
    }

    /// Used by attesters to get a proof that they were bonded before starting to produce attestations.
    /// A bonding proof is valid for `max_finality_period` blocks, the attester can only produce transition
    /// attestations for this specific amount of time.
    pub fn get_bond_proof(
        &self,
        address: S::Address,
        state: &mut WorkingSet<S>,
    ) -> StorageProof<<S::Storage as Storage>::Proof> {
        self.bonded_attesters.get_with_proof(&address, state)
    }

    /// TODO: Make the unbonding amount queryable:
    pub fn get_unbonding_amount(
        &self,
        _address: S::Address,
        _witness: &<S::Storage as Storage>::Witness,
    ) -> u64 {
        todo!("Make the unbonding amount queryable: https://github.com/Sovereign-Labs/sovereign-sdk/issues/675")
    }
}
