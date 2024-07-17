use std::io::Result;

/// The Nonces module does not support calls so we use [`NotInstantiable`] type here.
#[cfg_attr(
    feature = "native",
    derive(schemars::JsonSchema),
    derive(sov_modules_api::macros::CliWalletArg)
)]
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Clone)]
pub enum NotInstantiable {}

impl borsh::BorshDeserialize for NotInstantiable {
    // It is impossible to deserialize to NotInstantiable.
    fn deserialize_reader<R: std::io::prelude::Read>(_reader: &mut R) -> Result<Self> {
        panic!("NotInstantiable type cannot be deserialized")
    }
}

impl borsh::BorshSerialize for NotInstantiable {
    // Since it impossible to have a value of NotInstantiable this code is unreachable.
    fn serialize<W: std::io::Write>(&self, _writer: &mut W) -> Result<()> {
        unreachable!()
    }
}
