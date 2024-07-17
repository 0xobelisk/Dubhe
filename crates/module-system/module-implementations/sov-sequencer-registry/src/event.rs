use sov_modules_api::Spec;

/// Sample Event
#[derive(
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    PartialEq,
    Clone,
)]
pub enum Event<S: Spec> {
    /// A sequencer was registered.
    Registered {
        /// The address of the sequencer that was registered.
        sequencer: S::Address,
        /// The amount of the initial deposit.
        amount: u64,
    },

    /// A sequencer exited.
    Exited {
        /// The address of the sequencer that was exited.
        sequencer: S::Address,
    },

    /// A sequencer deposited funds to stake.
    Deposited {
        /// The address of the sequencer that was deposited to.
        sequencer: S::Address,
        /// The amount of the deposit.
        amount: u64,
    },
}
