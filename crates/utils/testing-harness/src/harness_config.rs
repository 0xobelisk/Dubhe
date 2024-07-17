use sov_celestia_adapter::CelestiaConfig;

use crate::utils::from_toml_path;

/// Just the DA part of full rollup_config.toml
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct HarnessConfig {
    pub(crate) da: CelestiaConfig,
}

impl HarnessConfig {
    pub(crate) fn from_toml_path<P: AsRef<std::path::Path>>(p: &P) -> anyhow::Result<Self> {
        from_toml_path(p)
    }
}
