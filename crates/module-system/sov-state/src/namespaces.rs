//! Defines namespaces that are used to partition the state of the rollup.

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    BorshSerialize,
    BorshDeserialize,
    Serialize,
    Deserialize,
)]
/// The namespaces used in the rollup. Related to the db's namespaces.
pub enum Namespace {
    /// The user namespace. Used by the User modules and is synchronised with the visible height.
    User,
    /// The kernel namespace. Used by the Kernel modules and is synchronised with the true height.
    Kernel,
    /// The accessory namespace. Values in this namespace are writeable but not readable inside the state transition
    /// function. They are used to provide auxiliary data via RPC.
    Accessory,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    BorshSerialize,
    BorshDeserialize,
    Serialize,
    Deserialize,
)]
/// Namespaces which can be merkle proven.
pub enum ProvableNamespace {
    /// The user namespace.
    User,
    /// The kernel namespace.
    Kernel,
}

/// Converts a type into a runtime namespace.
pub trait CompileTimeNamespace: core::fmt::Debug + Send + Sync + 'static {
    /// The runtime namespace variant associated with the type.
    const NAMESPACE: Namespace;
}

/// Converts a type into a Provable Namespace at compile time.
pub trait ProvableCompileTimeNamespace: CompileTimeNamespace {
    /// The runtime namespace variant associated with the type.
    const PROVABLE_NAMESPACE: ProvableNamespace;
}

/// A type-level representation of the user namespace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct User;

impl CompileTimeNamespace for User {
    const NAMESPACE: Namespace = Namespace::User;
}

impl ProvableCompileTimeNamespace for User {
    const PROVABLE_NAMESPACE: ProvableNamespace = ProvableNamespace::User;
}
/// A type-level representation of the kernel namespace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Kernel;

impl CompileTimeNamespace for Kernel {
    const NAMESPACE: Namespace = Namespace::Kernel;
}

impl ProvableCompileTimeNamespace for Kernel {
    const PROVABLE_NAMESPACE: ProvableNamespace = ProvableNamespace::Kernel;
}

/// A type-level representation of the accessory namespace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Accessory;

impl CompileTimeNamespace for Accessory {
    const NAMESPACE: Namespace = Namespace::Accessory;
}

#[derive(Default)]
/// A generic structure that divides a given type among the namespaces.
pub struct Namespaced<T> {
    user: T,
    kernel: T,
    accessory: T,
}

impl<T> Namespaced<T> {
    /// Gets the inner object for a given namespace.
    pub fn get(&self, namespace: Namespace) -> &T {
        match namespace {
            Namespace::User => &self.user,
            Namespace::Kernel => &self.kernel,
            Namespace::Accessory => &self.accessory,
        }
    }

    /// Gets the inner object for a given namespace.
    pub fn get_mut(&mut self, namespace: Namespace) -> &mut T {
        match namespace {
            Namespace::User => &mut self.user,
            Namespace::Kernel => &mut self.kernel,
            Namespace::Accessory => &mut self.accessory,
        }
    }

    /// Sets the inner object for a given namespace.
    pub fn set(&mut self, namespace: Namespace, state_update: T) {
        match namespace {
            Namespace::User => self.user = state_update,
            Namespace::Kernel => self.kernel = state_update,
            Namespace::Accessory => self.accessory = state_update,
        }
    }

    /// Creates a new struct instance from specified values.
    pub fn new(user: T, kernel: T, accessory: T) -> Self {
        Self {
            user,
            kernel,
            accessory,
        }
    }
}

impl<T> From<Namespaced<T>> for (T, T, T) {
    fn from(val: Namespaced<T>) -> Self {
        (val.user, val.kernel, val.accessory)
    }
}
