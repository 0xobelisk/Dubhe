/// Represents different types of events for NFT (Non-Fungible Token) operations.
#[derive(
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    PartialEq,
    Clone,
)]
/// Represents different types of events for NFT (Non-Fungible Token) operations.
pub enum Event {
    /// Event emitted when a new NFT is minted.
    ///
    /// Fields:
    /// - `id`: The token ID of the newly minted NFT.
    Mint {
        /// The unique identifier for the minted NFT.
        id: u64,
    },

    /// Event emitted when an NFT is burnt.
    ///
    /// Fields:
    /// - `id`: The token ID of the burnt NFT.
    Burn {
        /// The unique identifier for the burnt NFT.
        id: u64,
    },

    /// Event emitted when an NFT is transferred.
    ///
    /// Fields:
    /// - `id`: The token ID of the transferred NFT.
    Transfer {
        /// The unique identifier for the transferred NFT.
        id: u64,
    },
}
