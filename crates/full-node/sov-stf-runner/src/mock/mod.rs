use std::marker::PhantomData;

use sov_rollup_interface::da::{DaSpec, RelevantBlobIters};
use sov_rollup_interface::stf::{ApplySlotOutput, BatchReceipt, StateTransitionFunction};
use sov_rollup_interface::zk::{ValidityCondition, Zkvm};

/// A mock implementation of the [`StateTransitionFunction`]
#[derive(PartialEq, Debug, Clone, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct MockStf<Cond> {
    phantom_data: PhantomData<Cond>,
}

impl<InnerVm: Zkvm, OuterVm: Zkvm, Cond: ValidityCondition, Da: DaSpec>
    StateTransitionFunction<InnerVm, OuterVm, Da> for MockStf<Cond>
{
    type Address = Vec<u8>;
    type StateRoot = Vec<u8>;
    type GenesisParams = ();
    type PreState = ();
    type ChangeSet = ();
    type ProofReceiptContents = ();
    type TxReceiptContents = ();
    type BatchReceiptContents = ();
    type Witness = ();
    type Condition = Cond;

    // Perform one-time initialization for the genesis block.
    fn init_chain(
        &self,
        _base_state: Self::PreState,
        _params: Self::GenesisParams,
    ) -> (Self::StateRoot, ()) {
        (Vec::default(), ())
    }

    fn apply_slot<'a, I>(
        &self,
        _pre_state_root: &Self::StateRoot,
        _base_state: Self::PreState,
        _witness: Self::Witness,
        _slot_header: &Da::BlockHeader,
        _validity_condition: &Da::ValidityCondition,
        _relevant_blobs: RelevantBlobIters<I>,
    ) -> ApplySlotOutput<InnerVm, OuterVm, Da, Self>
    where
        I: IntoIterator<Item = &'a mut Da::BlobTransaction>,
    {
        ApplySlotOutput {
            state_root: Vec::default(),
            change_set: (),
            proof_receipts: vec![],
            batch_receipts: vec![BatchReceipt {
                batch_hash: [0; 32],
                tx_receipts: vec![],
                inner: (),
                gas_price: vec![],
            }],
            witness: (),
        }
    }
}
