//! Module error definitions.

/// A bech32 address parse error.
#[derive(Debug, thiserror::Error)]
pub enum Bech32ParseError {
    /// Bech32 decoding error represented via [bech32::primitives::decode::CheckedHrpstringError].
    #[error("Bech32 error: {0}")]
    Bech32(#[from] bech32::primitives::decode::CheckedHrpstringError),
    /// The provided "Human-Readable Part" is invalid.
    #[error("Wrong HRP: {0}")]
    WrongHRP(String),
}

/// General error type in the Module System.
#[derive(Debug, thiserror::Error)]
pub enum ModuleError {
    /// Custom error thrown by a module.
    #[error(transparent)]
    ModuleError(#[from] anyhow::Error),
}

impl serde::Serialize for ModuleError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let error = match self {
            ModuleError::ModuleError(e) => e.to_string(),
        };
        error.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for ModuleError {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let error = String::deserialize(deserializer)?;
        Ok(Self::ModuleError(anyhow::Error::msg(error)))
    }
}

impl Clone for ModuleError {
    fn clone(&self) -> Self {
        match self {
            Self::ModuleError(anyhow_err) => {
                Self::ModuleError(anyhow::Error::msg(anyhow_err.to_string()))
            }
        }
    }
}

impl PartialEq for ModuleError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::ModuleError(e1), Self::ModuleError(e2)) => e1.to_string() == e2.to_string(),
        }
    }
}

impl Eq for ModuleError {}

#[test]
fn test_module_error_roundtrip() {
    let error = ModuleError::ModuleError(anyhow::Error::msg("test"));
    let serialized = serde_json::to_string(&error).unwrap();
    let deserialized: ModuleError = serde_json::from_str(&serialized).unwrap();
    assert_eq!("test", deserialized.to_string());
}
