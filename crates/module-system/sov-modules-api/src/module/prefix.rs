use sha2::Digest;
use sov_rollup_interface::zk::CryptoSpec;
use sov_state::Prefix;

use crate::Spec;

// separator == "/"
const DOMAIN_SEPARATOR: [u8; 1] = [47];

/// A unique identifier for each state variable in a module.
#[derive(Debug, PartialEq, Eq)]
pub struct ModulePrefix {
    module_path: &'static str,
    module_name: &'static str,
    storage_name: Option<&'static str>,
}

impl ModulePrefix {
    /// Creates a new instance of a module prefix with the provided static definitions.
    pub fn new_storage(
        module_path: &'static str,
        module_name: &'static str,
        storage_name: &'static str,
    ) -> Self {
        Self {
            module_path,
            module_name,
            storage_name: Some(storage_name),
        }
    }

    /// Creates a new instance without a storage name.
    pub fn new_module(module_path: &'static str, module_name: &'static str) -> Self {
        Self {
            module_path,
            module_name,
            storage_name: None,
        }
    }

    /// Returns the parent module name.
    pub fn module_name(&self) -> &'static str {
        self.module_name
    }

    fn combine_prefix(&self) -> Vec<u8> {
        let storage_name_len = self
            .storage_name
            .map(|name| name.len().saturating_add(DOMAIN_SEPARATOR.len()))
            .unwrap_or_default();

        let mut combined_prefix = Vec::with_capacity(
            self.module_path
                .len()
                .saturating_add(self.module_name.len())
                .saturating_add(DOMAIN_SEPARATOR.len().saturating_mul(2))
                .saturating_add(storage_name_len),
        );

        combined_prefix.extend(self.module_path.as_bytes());
        combined_prefix.extend(DOMAIN_SEPARATOR);
        combined_prefix.extend(self.module_name.as_bytes());
        combined_prefix.extend(DOMAIN_SEPARATOR);
        if let Some(storage_name) = self.storage_name {
            combined_prefix.extend(storage_name.as_bytes());
            combined_prefix.extend(DOMAIN_SEPARATOR);
        }
        combined_prefix
    }

    /// Returns the hash of the combined prefix.
    pub fn hash<S: Spec>(&self) -> [u8; 32] {
        let combined_prefix = self.combine_prefix();
        let mut hasher = <S::CryptoSpec as CryptoSpec>::Hasher::new();
        hasher.update(combined_prefix);
        hasher.finalize().into()
    }
}

impl From<ModulePrefix> for Prefix {
    fn from(prefix: ModulePrefix) -> Self {
        let combined_prefix = prefix.combine_prefix();
        Prefix::new(combined_prefix)
    }
}
