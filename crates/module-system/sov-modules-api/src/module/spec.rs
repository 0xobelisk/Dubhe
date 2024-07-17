//! Module specification definitions.

use core::fmt::Debug;

use borsh::{BorshDeserialize, BorshSerialize};
use sov_rollup_interface::crypto::Signature;
use sov_rollup_interface::zk::{CryptoSpec, Zkvm};
use sov_rollup_interface::RollupAddress;
use sov_state::{Storage, Witness};

use crate::common::Gas;
use crate::higher_kinded_types::Generic;
use crate::transaction::Credentials;
use crate::{PublicKeyExt, SignatureExt};

/// The `Spec` trait configures certain key primitives to be used by a by a particular instance of a rollup.
/// `Spec` is almost always implemented on a Context object; since all Modules are generic
/// over a Context, rollup developers can easily optimize their code for different environments
/// by simply swapping out the Context (and by extension, the Spec).
///
/// For example, a rollup running in a STARK-based zkVM like Risc0 might pick Sha256 or Poseidon as its preferred hasher,
/// while a rollup running in an elliptic-curve based SNARK such as `Placeholder` from the =nil; foundation might
/// prefer a Pedersen hash. By using a generic Spec, a rollup developer can trivially customize their
/// code for either (or both!) of these environments without touching their module implementations.
pub trait Spec:
    BorshDeserialize
    + BorshSerialize
    + Default
    + Debug
    + Clone
    + Send
    + Sync
    + PartialEq
    + Generic
    + 'static
{
    /// Gas unit for the gas price computation.
    type Gas: Gas;

    /// The Address type used on the rollup. Typically calculated as the hash of a public key.
    #[cfg(feature = "native")]
    type Address: RollupAddress
        + BorshSerialize
        + BorshDeserialize
        + Sync
        + ::schemars::JsonSchema
        + for<'a> From<&'a <Self::CryptoSpec as CryptoSpec>::PublicKey>
        + std::str::FromStr<Err = anyhow::Error>;

    /// The Address type used on the rollup. Typically calculated as the hash of a public key.
    #[cfg(not(feature = "native"))]
    type Address: RollupAddress
        + BorshSerialize
        + BorshDeserialize
        + for<'a> From<&'a <Self::CryptoSpec as CryptoSpec>::PublicKey>;

    /// Authenticated state storage used by the rollup. Typically some variant of a merkle-patricia trie.
    #[cfg(not(feature = "native"))]
    type Storage: Storage + Send + Sync;

    /// Authenticated state storage used by the rollup. Typically some variant of a merkle-patricia trie.
    #[cfg(feature = "native")]
    type Storage: Storage + sov_state::NativeStorage + Send + Sync;

    /// The Zkvm which verifies the inner circuit, where
    /// the `inner` circuit proves the correctness of the state transition for individual DA blocks.
    type InnerZkvm: Zkvm;

    /// The Zkvm which verifies the outer circuit, where
    /// the `outer` circuit proves the correctness of the state transition for the whole chain since genesis.
    type OuterZkvm: Zkvm;

    /// The hash type accessible by the execution environment of the rollup.
    /// In the case of a rollup compatible with soft-confirmations, this is the hash of the `User` space.
    /// In all the other cases it is the same as the [`Storage::Root`] associated type.
    type VisibleHash: Into<[u8; 32]> + From<<Self::Storage as Storage>::Root>;

    /// The cryptographic primitives used by the rollup.
    type CryptoSpec: CryptoSpecExt;

    /// A structure containing the non-deterministic inputs from the prover to the zk-circuit
    type Witness: Witness;
}

/// A helper trait which is blanket implemented for all `CryptoSpec` types that
/// are also compatible with module system requirements. This helper works around the lack of implied bounds in Rustc.
/// See <https://github.com/rust-lang/rust/issues/121325> for details.
#[cfg(not(feature = "native"))]
pub trait CryptoHelper:
    CryptoSpec<Signature = Self::ExtendedSignature, PublicKey = Self::ExtendedPublicKey>
{
    /// The digital signature scheme used by the rollup.
    type ExtendedSignature: SignatureExt + Signature<PublicKey = Self::ExtendedPublicKey>;

    /// The public key used for digital signatures
    type ExtendedPublicKey: PublicKeyExt;
}

/// A helper trait which is blanket implemented for all `CryptoSpec` types that
/// are also compatible with module system requirements. This helper works around the lack of implied bounds in Rustc.
/// See <https://github.com/rust-lang/rust/issues/121325> for details.
#[cfg(feature = "native")]
pub trait CryptoHelper:
    CryptoSpec<
    Signature = Self::ExtendedSignature,
    PublicKey = Self::ExtendedPublicKey,
    PrivateKey = Self::ExtendedPrivateKey,
>
{
    /// The digital signature scheme used by the rollup.
    type ExtendedSignature: SignatureExt + Signature<PublicKey = Self::ExtendedPublicKey>;

    /// The public key used for digital signatures
    type ExtendedPublicKey: PublicKeyExt;

    /// The private key used for digital signatures
    type ExtendedPrivateKey: crate::PrivateKeyExt<
        PublicKey = Self::ExtendedPublicKey,
        Signature = Self::ExtendedSignature,
    >;
}

/// An extension trait for a `CryptoSpec` which guarantees that the type implements the
/// slightly more restrictive traits defined in the module system.
pub trait CryptoSpecExt: CryptoHelper {}

#[cfg(feature = "native")]
impl<C: CryptoSpec> CryptoHelper for C
where
    C::Signature: SignatureExt,
    C::PublicKey: PublicKeyExt,
    C::PrivateKey: crate::PrivateKeyExt,
{
    type ExtendedPrivateKey = C::PrivateKey;
    type ExtendedSignature = C::Signature;
    type ExtendedPublicKey = C::PublicKey;
}

#[cfg(not(feature = "native"))]
impl<C: CryptoSpec> CryptoHelper for C
where
    C::Signature: SignatureExt,
    C::PublicKey: PublicKeyExt,
{
    type ExtendedPublicKey = C::PublicKey;
    type ExtendedSignature = C::Signature;
}

/// An extension trait for a `CryptoSpec` which guarantees that the type implements the
/// slightly more restrictive traits defined in the module system.
impl<C: CryptoHelper> CryptoSpecExt for C {}

/// The context in which a transaction executes

#[derive(Clone, Debug)]
pub struct Context<S: Spec> {
    /// The original credentials of the sender.
    sender_credentials: Credentials,
    /// The sender address of the transaction.
    sender: S::Address,
    /// The rollup address of the sequencer who included the transaction.
    sequencer: S::Address,
    /// The height to report. This is set by the kernel when the context is created
    visible_height: u64,
    phantom: core::marker::PhantomData<S>,
}

impl<S: Spec> Context<S> {
    /// Returns the rollup address which sent the transaction.
    pub fn sender(&self) -> &S::Address {
        &self.sender
    }

    /// Returns the rollup address of the sequencer which included the transaction.
    pub fn sequencer(&self) -> &S::Address {
        &self.sequencer
    }

    /// Constructs a new Context.
    pub fn new(
        sender: S::Address,
        sender_credentials: Credentials,
        sequencer: S::Address,
        height: u64,
    ) -> Self {
        Self {
            sender_credentials,
            sender,
            sequencer,
            visible_height: height,
            phantom: core::marker::PhantomData,
        }
    }

    /// Returns the sender's credentials.
    pub fn get_sender_credential<T: core::any::Any>(&self) -> Option<&T> {
        self.sender_credentials.get::<T>()
    }

    /// Returns the current slot number.
    pub fn visible_slot_number(&self) -> u64 {
        self.visible_height
    }
}

#[cfg(feature = "arbitrary")]
mod arbitrary {
    use ::arbitrary::{Arbitrary, Unstructured};

    use super::{Context, Spec};
    impl<'a, S> Arbitrary<'a> for Context<S>
    where
        S: Spec,
        S::Address: Arbitrary<'a>,
    {
        fn arbitrary(u: &mut Unstructured<'a>) -> ::arbitrary::Result<Self> {
            let sender = u.arbitrary()?;
            let sequencer = u.arbitrary()?;
            let height = u.arbitrary()?;
            Ok(Self::new(sender, Default::default(), sequencer, height))
        }
    }
}
