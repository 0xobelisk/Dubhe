use anyhow::Result;
use sov_modules_api::GenesisState;

use super::ValueSetter;

/// Initial configuration for sov-value-setter module.
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
#[cfg_attr(
    feature = "native",
    derive(schemars::JsonSchema),
    schemars(bound = "S: ::sov_modules_api::Spec", rename = "ValueSetterConfig")
)]
pub struct ValueSetterConfig<S: sov_modules_api::Spec> {
    /// Admin of the module.
    pub admin: S::Address,
}

impl<S: sov_modules_api::Spec> ValueSetter<S> {
    /// Initializes module with the `admin` role.
    pub(crate) fn init_module(
        &self,
        admin_config: &<Self as sov_modules_api::Module>::Config,
        state: &mut impl GenesisState<S>,
    ) -> Result<()> {
        self.admin.set(&admin_config.admin, state)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sov_modules_api::prelude::serde_json;
    use sov_modules_api::Address;
    use sov_test_utils::TestSpec;

    use crate::ValueSetterConfig;

    #[test]
    fn test_config_serialization() {
        let admin = Address::from([1; 32]);
        let config = ValueSetterConfig::<TestSpec> { admin };

        let data = r#"
        {
            "admin":"sov1qyqszqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgpqyqs259tk3"
        }"#;

        let parsed_config: ValueSetterConfig<TestSpec> = serde_json::from_str(data).unwrap();
        assert_eq!(parsed_config, config);
    }
}
