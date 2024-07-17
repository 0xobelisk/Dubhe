use std::collections::HashMap;

use serde::Serialize;

use super::{Prefix, StateItemInfo};
use crate::{ModuleId, ModuleInfo};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum StateItemContents<K, V> {
    Value { value: Option<V> },
    Vec { length: usize },
    VecElement { index: usize, value: Option<V> },
    MapElement { key: K, value: Option<V> },
}

/// Identical to [`sov_state::namespaces::Namespace`], but with a custom
/// [`serde`] implementation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")] // <-- This is the important difference.
pub enum Namespace {
    User,
    Kernel,
    Accessory,
}

impl From<sov_state::namespaces::Namespace> for Namespace {
    fn from(value: sov_state::namespaces::Namespace) -> Self {
        match value {
            sov_state::namespaces::Namespace::User => Self::User,
            sov_state::namespaces::Namespace::Kernel => Self::Kernel,
            sov_state::namespaces::Namespace::Accessory => Self::Accessory,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type", rename = "module")]
pub struct ModuleObject {
    pub id: ModuleId,
    pub name: String,
    pub description: Option<String>,
    pub prefix: Prefix,
    pub state_items: HashMap<String, StateItemInfo>,
}

impl ModuleObject {
    pub fn new(
        module: &(impl ModuleInfo + ?Sized),
        description: Option<String>,
        state_items: HashMap<String, StateItemInfo>,
    ) -> Self {
        Self {
            id: *module.id(),
            description,
            name: module.prefix().module_name().to_owned(),
            prefix: Prefix(module.prefix().into()),
            state_items,
        }
    }
}
