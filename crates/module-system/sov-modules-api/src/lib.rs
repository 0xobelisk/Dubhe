#![doc = include_str!("../README.md")]

mod batch;
#[cfg(feature = "native")]
pub mod cli;
pub mod common;
mod containers;
pub mod default_spec;
pub mod higher_kinded_types;
pub mod hooks;
pub mod module;
#[cfg(feature = "native")]
pub mod rest;
#[cfg(feature = "native")]
pub mod rpc;
pub mod runtime;
pub mod state;

pub use batch::*;
pub use common::*;
pub use module::*;
#[cfg(feature = "native")]
pub use rpc::*;
pub use runtime::*;
pub use state::*;

pub mod proof_metadata;

mod reexport_macros;
pub use reexport_macros::*;

#[cfg(test)]
mod tests;
pub mod transaction;
#[cfg(feature = "native")]
pub mod utils;
use std::collections::{HashMap, HashSet};

#[cfg(feature = "native")]
pub use clap;
pub use containers::*;
#[cfg(feature = "native")]
pub use schemars;
#[cfg(feature = "native")]
pub use sov_rollup_interface::crypto::PrivateKey;
pub use sov_rollup_interface::crypto::{CredentialId, PublicKey, Signature};
pub use sov_rollup_interface::da::{BlobReaderTrait, DaSpec};
#[cfg(feature = "native")]
pub use sov_rollup_interface::services::da::SlotData;
pub use sov_rollup_interface::stf::*;
pub use sov_rollup_interface::zk::aggregated_proof::{AggregatedProofPublicData, CodeCommitment};
pub use sov_rollup_interface::zk::{
    CryptoSpec, StateTransitionPublicData, ValidityCondition, ValidityConditionChecker, Zkvm,
};
pub use sov_rollup_interface::{digest, execution_mode, BasicAddress, RollupAddress};
pub use sov_state::Storage;

pub use crate::common::ModuleError as Error;
pub use crate::proof_metadata::SovApiProofSerializer;
pub use crate::state::StateReaderAndWriter;
pub mod optimistic {
    pub use sov_rollup_interface::optimistic::{Attestation, ProofOfBond};
}

pub mod da {
    pub use sov_rollup_interface::da::{BlockHeaderTrait, NanoSeconds, Time};
}

/// Prelude with re-exports of external crates used by macros, as well as
/// important traits and types.
///
/// This is meant to be "glob" imported wherever you'll use
/// [`sov_modules_api::macros`](crate::macros).
///
/// ```rust
/// use sov_modules_api::prelude::*;
/// use sov_modules_api::macros::CliWalletArg;
///
/// #[derive(CliWalletArg)]
/// struct MyStruct;
/// ```
pub mod prelude {
    // A NOTE ABOUT PRELUDES
    // ---------------------
    // I'm generally against preludes in Rust code, and the rest of the Rust
    // community seems to agree. I believe our use case warrants a prelude, though,
    // as we reexport many macros from many different crates, and a "glob" import is
    // the only way for us to inject all of these dependencies into downstream code
    // without its authors having to mess with their Cargo manifests.
    //
    // There's another thing to consider. Oftentimes, downstream code will only ever
    // use proc-macros when the `native` Cargo feature is enabled, which would mean
    // that the prelude "glob" import generates an "unused import" warning when
    // `native` is disabled. To avoid this, it's a good idea for us to also re-export
    // some other items that will (almost) always be used regardless of Cargo
    // features. `Spec`, for example, fits the bill. This ensures the lowest
    // possible amount of warnings, and thus the amount of linting exceptions
    // that users will have to deal with.
    //
    // In some cases, it's easy for proc-macros to depend on dependencies
    // re-exported here. In others, however, the original proc-macro references
    // its dependency with an absolute path, e.g. `::serde`. There is,
    // unfortunately, no way around that, unless said proc-macro also allows
    // configuring the crate path, e.g.
    // `#[serde(crate = "sov_modules_api::prelude::serde")]`
    //
    // This means that, in practice, re-exporting proc-macros is often difficult
    // or can't always be done.

    pub use crate::macros::*;
    #[cfg(feature = "native")]
    pub use crate::rest::ModuleRestApi;
    pub use crate::{
        Context, DaSpec, ModuleCallJsonSchema, Spec, StateAccessor, StateReaderAndWriter,
        WorkingSet,
    };
    pub extern crate tracing;

    pub extern crate anyhow;
    #[cfg(feature = "native")]
    pub extern crate axum;
    pub extern crate bech32;
    #[cfg(feature = "native")]
    pub extern crate clap;
    pub extern crate serde;
    #[cfg(feature = "native")]
    pub extern crate serde_json;
    #[cfg(feature = "native")]
    pub extern crate sov_rest_utils;
    #[cfg(feature = "native")]
    pub extern crate tokio;
    #[cfg(feature = "native")]
    pub extern crate utoipa;
    #[cfg(feature = "native")]
    pub extern crate utoipa_swagger_ui;

    pub use unwrap_infallible::UnwrapInfallible;
}

struct ModuleVisitor<'a, S: Spec> {
    visited: HashSet<&'a ModuleId>,
    visited_on_this_path: Vec<&'a ModuleId>,
    sorted_modules: std::vec::Vec<&'a dyn ModuleInfo<Spec = S>>,
}

impl<'a, S: Spec> ModuleVisitor<'a, S> {
    pub fn new() -> Self {
        Self {
            visited: HashSet::new(),
            sorted_modules: Vec::new(),
            visited_on_this_path: Vec::new(),
        }
    }

    /// Visits all the modules and their dependencies, and populates a Vec of modules sorted by their dependencies
    fn visit_modules(
        &mut self,
        modules: Vec<&'a dyn ModuleInfo<Spec = S>>,
    ) -> Result<(), anyhow::Error> {
        let mut module_map = HashMap::new();

        for module in &modules {
            module_map.insert(module.id(), *module);
        }

        for module in modules {
            self.visited_on_this_path.clear();
            self.visit_module(module, &module_map)?;
        }

        Ok(())
    }

    /// Visits a module and its dependencies, and populates a Vec of modules sorted by their dependencies
    fn visit_module(
        &mut self,
        module: &'a dyn ModuleInfo<Spec = S>,
        module_map: &HashMap<&'a ModuleId, &'a (dyn ModuleInfo<Spec = S>)>,
    ) -> Result<(), anyhow::Error> {
        let id = module.id();

        // if the module have been visited on this path, then we have a cycle dependency
        if let Some((index, _)) = self
            .visited_on_this_path
            .iter()
            .enumerate()
            .find(|(_, &x)| x == id)
        {
            let cycle = &self.visited_on_this_path[index..];

            anyhow::bail!(
                "Cyclic dependency of length {} detected: {:?}",
                cycle.len(),
                cycle
            );
        } else {
            self.visited_on_this_path.push(id);
        }

        // if the module hasn't been visited yet, visit it and its dependencies
        if self.visited.insert(id) {
            for dependency_address in module.dependencies() {
                let dependency_module = *module_map.get(&dependency_address).ok_or_else(|| {
                    anyhow::Error::msg(format!("Module not found: {:?}", dependency_address))
                })?;
                self.visit_module(dependency_module, module_map)?;
            }

            self.sorted_modules.push(module);
        }

        // remove the module from the visited_on_this_path list
        self.visited_on_this_path.pop();

        Ok(())
    }
}

/// Sorts ModuleInfo objects by their dependencies
fn sort_modules_by_dependencies<S: Spec>(
    modules: Vec<&dyn ModuleInfo<Spec = S>>,
) -> Result<Vec<&dyn ModuleInfo<Spec = S>>, anyhow::Error> {
    let mut module_visitor = ModuleVisitor::<S>::new();
    module_visitor.visit_modules(modules)?;
    Ok(module_visitor.sorted_modules)
}

/// Accepts Vec<> of tuples (&ModuleInfo, &TValue), and returns Vec<&TValue> sorted by mapped module dependencies
pub fn sort_values_by_modules_dependencies<S: Spec, TValue>(
    module_value_tuples: Vec<(&dyn ModuleInfo<Spec = S>, TValue)>,
) -> Result<Vec<TValue>, anyhow::Error>
where
    TValue: Clone,
{
    let sorted_modules = sort_modules_by_dependencies(
        module_value_tuples
            .iter()
            .map(|(module, _)| *module)
            .collect(),
    )?;

    let mut value_map = HashMap::new();

    for module in module_value_tuples {
        let prev_entry = value_map.insert(module.0.id(), module.1);
        anyhow::ensure!(prev_entry.is_none(), "Duplicate module id! Only one instance of each module is allowed in a given runtime. Module with ID {} is duplicated", module.0.id());
    }

    let mut sorted_values = Vec::new();
    for module in sorted_modules {
        sorted_values.push(value_map.get(&module.id()).unwrap().clone());
    }

    Ok(sorted_values)
}

/// This trait is implemented by types that can be used as arguments in the sov-cli wallet.
/// The recommended way to implement this trait is using the provided derive macro (`#[derive(CliWalletArg)]`).
/// Currently, this trait is a thin wrapper around [`clap::Parser`]
#[cfg(feature = "native")]
pub trait CliWalletArg: From<Self::CliStringRepr> {
    /// The type that is used to represent this type in the CLI. Typically,
    /// this type implements the clap::Subcommand trait.
    type CliStringRepr;
}

/// A trait that needs to be implemented for a *runtime* to be used with the CLI wallet
#[cfg(feature = "native")]
pub trait CliWallet: DispatchCall {
    /// The type that is used to represent this type in the CLI. Typically,
    /// this type implements the clap::Subcommand trait. This type is generic to
    /// allow for different representations of the same type in the interface; a
    /// typical end-usage will impl traits only in the case where `CliStringRepr<T>: Into::RuntimeCall`
    type CliStringRepr<T>;
}

#[doc(hidden)]
#[cfg(feature = "native")]
pub mod __rpc_macros_private {
    use super::*;

    /// A [`Module`] that also exposes a JSON-RPC server.
    pub trait ModuleWithRpcServer {
        type Spec: Spec;

        fn rpc_methods(
            &self,
            storage: tokio::sync::watch::Receiver<<Self::Spec as Spec>::Storage>,
        ) -> jsonrpsee::RpcModule<()>;
    }

    // Auto-ref trick so that implementing JSON-RPC for modules is effectively
    // optional.
    impl<M> ModuleWithRpcServer for &M
    where
        M: ModuleInfo,
    {
        type Spec = M::Spec;

        fn rpc_methods(
            &self,
            _storage: tokio::sync::watch::Receiver<<Self::Spec as Spec>::Storage>,
        ) -> jsonrpsee::RpcModule<()> {
            jsonrpsee::RpcModule::new(())
        }
    }
}
