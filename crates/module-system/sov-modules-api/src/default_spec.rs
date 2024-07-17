use borsh::{BorshDeserialize, BorshSerialize};
use sov_rollup_interface::execution_mode;
#[cfg(feature = "native")]
use sov_rollup_interface::execution_mode::{Native, WitnessGeneration};
use sov_rollup_interface::zk::{CryptoSpec, Zkvm};
use sov_state::{ArrayWitness, DefaultStorageSpec};

use crate::higher_kinded_types::{Generic, HigherKindedHelper};
use crate::{Address, GasUnit, Spec};

#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Default, serde::Serialize, serde::Deserialize, BorshDeserialize, BorshSerialize)]
#[serde(bound = "")]
pub struct DefaultSpec<InnerZkvm, OuterZkvm, Mode>(
    std::marker::PhantomData<(InnerZkvm, OuterZkvm, Mode)>,
);

impl<InnerZkvm: Zkvm, OuterZkvm: Zkvm, M> Generic for DefaultSpec<InnerZkvm, OuterZkvm, M> {
    type With<K> = DefaultSpec<InnerZkvm, OuterZkvm, K>;
}

impl<InnerZkvm: Zkvm, OuterZkvm: Zkvm, M> HigherKindedHelper
    for DefaultSpec<InnerZkvm, OuterZkvm, M>
{
    type Inner = M;
}

mod default_impls {
    use sov_rollup_interface::execution_mode::ExecutionMode;

    use super::DefaultSpec;

    impl<InnerZkvm, OuterZkvm, Mode: ExecutionMode> Clone for DefaultSpec<InnerZkvm, OuterZkvm, Mode> {
        fn clone(&self) -> Self {
            Self(std::marker::PhantomData)
        }
    }

    impl<InnerZkvm, OuterZkvm, Mode: ExecutionMode> PartialEq<Self>
        for DefaultSpec<InnerZkvm, OuterZkvm, Mode>
    {
        fn eq(&self, _other: &Self) -> bool {
            true
        }
    }

    impl<InnerZkvm, OuterZkvm, Mode: ExecutionMode> core::fmt::Debug
        for DefaultSpec<InnerZkvm, OuterZkvm, Mode>
    {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(
                f,
                "DefaultSpec<{}>",
                std::any::type_name::<(InnerZkvm, OuterZkvm, Mode)>()
            )
        }
    }
}

#[cfg(feature = "native")]
impl<InnerZkvm: Zkvm, OuterZkvm: Zkvm> Spec for DefaultSpec<InnerZkvm, OuterZkvm, WitnessGeneration>
where
    InnerZkvm::CryptoSpec: crate::CryptoSpecExt,
{
    type Address = Address<<Self::CryptoSpec as CryptoSpec>::Hasher>;
    type Gas = GasUnit<2>;

    type Storage =
        sov_state::ProverStorage<DefaultStorageSpec<<Self::CryptoSpec as CryptoSpec>::Hasher>>;

    type VisibleHash = sov_state::VisibleHash;

    type InnerZkvm = InnerZkvm;
    type OuterZkvm = OuterZkvm;

    type CryptoSpec = InnerZkvm::CryptoSpec;

    type Witness = ArrayWitness;
}

#[cfg(feature = "native")]
impl<InnerZkvm: Zkvm, OuterZkvm: Zkvm> Spec for DefaultSpec<InnerZkvm, OuterZkvm, Native>
where
    InnerZkvm::CryptoSpec: crate::CryptoSpecExt,
{
    type Address = Address<<Self::CryptoSpec as CryptoSpec>::Hasher>;
    type Gas = GasUnit<2>;

    // TODO: Replace ProverStorage with an optimized impl!
    type Storage =
        sov_state::ProverStorage<DefaultStorageSpec<<Self::CryptoSpec as CryptoSpec>::Hasher>>;

    type VisibleHash = sov_state::VisibleHash;

    type InnerZkvm = InnerZkvm;
    type OuterZkvm = OuterZkvm;

    type CryptoSpec = InnerZkvm::CryptoSpec;

    // TODO: Replace Array witness with an empty struct
    type Witness = ArrayWitness;
}

impl<InnerZkvm: Zkvm, OuterZkvm: Zkvm> Spec
    for DefaultSpec<InnerZkvm, OuterZkvm, execution_mode::Zk>
where
    InnerZkvm::CryptoSpec: crate::CryptoSpecExt,
{
    type Address = Address<<Self::CryptoSpec as CryptoSpec>::Hasher>;
    type Gas = GasUnit<2>;

    type Storage =
        sov_state::ZkStorage<DefaultStorageSpec<<Self::CryptoSpec as CryptoSpec>::Hasher>>;

    type VisibleHash = sov_state::VisibleHash;

    type InnerZkvm = InnerZkvm;
    type OuterZkvm = OuterZkvm;

    type CryptoSpec = InnerZkvm::CryptoSpec;

    type Witness = ArrayWitness;
}
