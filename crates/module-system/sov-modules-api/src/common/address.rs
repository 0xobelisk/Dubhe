//! Module id definitions

#[cfg(feature = "arbitrary")]
use arbitrary::{Arbitrary, Unstructured};
use borsh::{BorshDeserialize, BorshSerialize};
use derivative::Derivative;
use sha2::digest::typenum::U32;
use sha2::Digest;
use sov_rollup_interface::crypto::PublicKey;
use sov_rollup_interface::{BasicAddress, RollupAddress};

/// Implement type conversions between a `\[u8;32\]` wrapper and a bech32 string representation.
/// This implementation assumes that the wrapper implents a `fn as_bytes(&self) -> &[u8; 32]` as
/// well as `From<\[u8;32\]>` and `AsRef<[u8]>`.
#[macro_export]
macro_rules! impl_bech32_conversion {
    // We make this function generic because the Address type will eventually need a generic
    ($id:ident $( < $generic:ident >)?, $bech32_version:ident, $human_readable_prefix:expr) => {
        /// A pre-validated bech32 representation of $id
        #[derive(
            serde::Serialize,
            serde::Deserialize,
            borsh::BorshDeserialize,
            borsh::BorshSerialize,
            Debug,
            PartialEq,
            Clone,
            Eq,
        )]
        #[serde(try_from = "String", into = "String")]
        pub struct $bech32_version (
            /// A validated bech32 string
            String,
        );

        const __BECH32_HRP: &str = $human_readable_prefix;

        mod __bech32_conversion_impls {
            use super:: $id;
            use super:: $bech32_version;
            use super:: __BECH32_HRP;
            use std::fmt;
            use std::str::FromStr;
            use $crate::prelude::{bech32, serde, anyhow};
            use bech32::primitives::decode::{UncheckedHrpstring, CheckedHrpstring};
            use bech32::{Bech32m, Hrp};

            impl From<$bech32_version> for String {
                fn from(bech: $bech32_version) -> Self {
                    bech.0
                }
            }

            impl core::fmt::Display for $bech32_version {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    write!(f, "{}", self.0)
                }
            }

            impl $(< $generic > )? FromStr for $id $(< $generic > )?{
                type Err = anyhow::Error;

                fn from_str(s: &str) -> Result<Self, Self::Err> {
                    $bech32_version::from_str(s)
                        .map_err(|e| anyhow::anyhow!(e))
                        .map(|item_bech32| item_bech32.into())
                }
            }


            impl FromStr for $bech32_version {
                type Err = $crate::common::Bech32ParseError;

                fn from_str(s: &str) -> Result<Self, $crate::common::Bech32ParseError> {
                    let hrp_string = CheckedHrpstring::new::<Bech32m>(s)?;

                    if hrp_string.hrp().as_str() != __BECH32_HRP {
                        return Err($crate::common::Bech32ParseError::WrongHRP(hrp_string.hrp().to_string()));
                    }

                    Ok($bech32_version (
                        s.to_string(),
                    ))
                }
            }

            impl $(< $generic > )? fmt::Display for $id $(< $generic > )? {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    write!(f, "{}", $bech32_version::from(self))
                }
            }

            impl $(< $generic > )? fmt::Debug for $id $(< $generic > )? {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    write!(f, "{:?}", $bech32_version::from(self))
                }
            }

            impl $(< $generic > )? From<$bech32_version> for $id $(< $generic > )? {
                fn from(addr: $bech32_version) -> Self {
                    addr.to_byte_array().into()
                }
            }

            impl $(< $generic > )? serde::Serialize for $id $(< $generic > )?  {
                fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where
                    S: serde::Serializer,
                {
                    if serializer.is_human_readable() {
                        serde::Serialize::serialize(& $bech32_version::from(self), serializer)
                    } else {
                        serde::Serialize::serialize(self.as_bytes(), serializer)
                    }
                }
            }

            impl<'de $(, $generic)?> serde::Deserialize<'de> for $id $(< $generic > )? {
                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where
                    D: serde::Deserializer<'de>,
                {
                    if deserializer.is_human_readable() {
                        let bech: $bech32_version = serde::Deserialize::deserialize(deserializer)?;
                        Ok($id::from(bech.to_byte_array()))
                    } else {
                        let bytes = <[u8; 32] as serde::Deserialize>::deserialize(deserializer)?;
                        Ok(bytes.into())
                    }
                }
            }

            impl $bech32_version {
                pub(crate) fn to_byte_array(&self) -> [u8; 32] {
                    let hrp_string = UncheckedHrpstring::new(&self.0)
                        .expect("Bech32 was validated at construction")
                        .remove_checksum::<Bech32m>();

                    let mut addr_bytes = [0u8; 32];
                    for (l, r) in addr_bytes.iter_mut().zip(hrp_string.byte_iter()) {
                        *l = r;
                    }
                    addr_bytes
                }

                /// Returns the human readable prefix for the bech32 representation
                pub fn human_readable_prefix() -> &'static str {
                    __BECH32_HRP
                }
            }

            impl $(< $generic > )? From<$id $(< $generic > )?> for $bech32_version {
                fn from(addr: $id $(< $generic > )?) -> Self {
                    let string = bech32::encode::<Bech32m>(Hrp::parse_unchecked(__BECH32_HRP), addr.as_ref()).expect("Encoding to string is infallible");
                    $bech32_version(string)
                }
            }


            impl $(< $generic > )? From<& $id $(< $generic > )?> for $bech32_version {
                fn from(addr: & $id $(< $generic > )?) -> Self {
                    let string = bech32::encode::<Bech32m>(Hrp::parse_unchecked(__BECH32_HRP), addr.as_ref()).expect("Encoding to string is infallible");
                    $bech32_version(string)
                }
            }


            impl TryFrom<String> for $bech32_version {
                type Error = $crate::common::Bech32ParseError;

                fn try_from(addr: String) -> Result<Self, $crate::common::Bech32ParseError> {
                    $bech32_version::from_str(&addr)
                }
            }
        }

    };
}

#[macro_export]
/// Implements a newtype around `\[u8;32\]` which can be displayed in bech32 format with the provided
/// human readable prefix.
macro_rules! impl_hash32_type {
    ($id:ident, $bech32_version:ident, $human_readable_prefix:expr) => {
        #[derive(
            Clone, Copy, PartialEq, Eq, Hash, borsh::BorshDeserialize, borsh::BorshSerialize,
        )]
        #[cfg_attr(feature = "native", derive(schemars::JsonSchema))]
        #[cfg_attr(feature = "arbitrary", derive(proptest_derive::Arbitrary))]
        /// A globally unique identifier.
        pub struct $id([u8; 32]);

        impl From<[u8; 32]> for $id {
            fn from(inner: [u8; 32]) -> Self {
                Self(inner)
            }
        }

        impl AsRef<[u8]> for $id {
            fn as_ref(&self) -> &[u8] {
                &self.0
            }
        }

        impl<'a> TryFrom<&'a [u8]> for $id {
            type Error = anyhow::Error;

            fn try_from(id: &'a [u8]) -> Result<Self, Self::Error> {
                if id.len() != 32 {
                    anyhow::bail!("Id must be 32 bytes long");
                }
                let mut id_bytes = [0u8; 32];
                id_bytes.copy_from_slice(id);
                Ok(Self(id_bytes))
            }
        }

        impl $id {
            /// Exposes the inner bytes of $id
            pub const fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }

            /// Converts the id to a bech32 string
            pub fn to_bech32(&self) -> $bech32_version {
                self.into()
            }

            /// Returns the human readable prefix for the bech32 representation
            pub const fn bech32_prefix() -> &'static str {
                $human_readable_prefix
            }

            /// Creates a new $id containing the given bytes. This function is needed in addition
            /// to the `From` trait to allow for const conversions
            pub const fn from_const_slice(addr: [u8; 32]) -> Self {
                Self(addr)
            }
        }

        $crate::impl_bech32_conversion!($id, $bech32_version, $human_readable_prefix);
    };
}

impl_bech32_conversion!(Address<H>, AddressBech32, ADDRESS_PREFIX);

/// Module ID representation.
#[cfg_attr(feature = "arbitrary", derive(proptest_derive::Arbitrary))]
#[derive(Derivative, BorshDeserialize, BorshSerialize)]
#[derivative(Copy, Hash, PartialEq, Eq)]
pub struct Address<H> {
    addr: [u8; 32],
    #[derivative(Hash = "ignore", PartialEq = "ignore")]
    phantom: std::marker::PhantomData<H>,
}

// We manually implement clone so that we can silence this clippy warning.
// Derivative has o facility to enable that.
#[allow(clippy::non_canonical_clone_impl)]
impl<H> Clone for Address<H> {
    fn clone(&self) -> Self {
        Self {
            addr: self.addr,
            phantom: std::marker::PhantomData,
        }
    }
}

#[cfg(feature = "native")]
impl<H> schemars::JsonSchema for Address<H> {
    fn schema_name() -> String {
        "Address".to_string()
    }

    fn json_schema(_gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "string",
            // TODO(@neysofu): this regex pattern is currently correct, but it
            // must be updated if `Address` allows for custom prefixes, instead
            // of hardcoding `sov`.
            "pattern": "^sov1[a-zA-Z0-9]+$",
            "description": "Address",
        }))
        .unwrap()
    }
}

impl<H: Digest<OutputSize = U32>, T: PublicKey> From<&T> for Address<H> {
    fn from(value: &T) -> Self {
        value.credential_id::<H>().0.into()
    }
}

impl<H> AsRef<[u8]> for Address<H> {
    fn as_ref(&self) -> &[u8] {
        &self.addr
    }
}

impl<H> Address<H> {
    /// Creates a new address containing the given bytes
    pub const fn new(addr: [u8; 32]) -> Self {
        Self {
            addr,
            phantom: std::marker::PhantomData,
        }
    }

    /// Exposes the inner bytes of the Address
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.addr
    }
}

impl<'a, H> TryFrom<&'a [u8]> for Address<H> {
    type Error = anyhow::Error;

    fn try_from(addr: &'a [u8]) -> Result<Self, Self::Error> {
        if addr.len() != 32 {
            anyhow::bail!("Address must be 32 bytes long");
        }
        let mut addr_bytes = [0u8; 32];
        addr_bytes.copy_from_slice(addr);
        Ok(Self {
            addr: addr_bytes,
            phantom: std::marker::PhantomData,
        })
    }
}

impl<H> From<[u8; 32]> for Address<H> {
    fn from(addr: [u8; 32]) -> Self {
        Self {
            addr,
            phantom: std::marker::PhantomData,
        }
    }
}

impl<H> Address<H> {
    /// Creates a new $id containing the given bytes. This function is needed in addition
    /// to the `From` trait to allow for const conversions
    pub const fn from_const_slice(addr: [u8; 32]) -> Self {
        Self {
            addr,
            phantom: std::marker::PhantomData,
        }
    }
}

// Implement arbitrary by hand because the derive can't handle PhantomData
#[cfg(feature = "arbitrary")]
impl<'a, H> Arbitrary<'a> for Address<H> {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let addr = u.arbitrary()?;
        Ok(Self {
            addr,
            phantom: std::marker::PhantomData,
        })
    }
}

impl<H: Send + Sync + 'static> BasicAddress for Address<H> {}
impl<H: Send + Sync + 'static> RollupAddress for Address<H> {}

// TODO(@preston-evans98): unify core and modules, then
// enable sov-modules-macros and do this
// #[sov_modules_macros::config_constant]
const ADDRESS_PREFIX: &str = "sov";

#[cfg(test)]
mod test {

    use core::str::FromStr;

    use sha2::Sha256;

    use super::*;

    #[test]
    fn test_address_serialization() {
        let address = Address::from([11; 32]);
        let data: String = serde_json::to_string(&address).unwrap();
        let deserialized_address = serde_json::from_str::<Address<Sha256>>(&data).unwrap();

        assert_eq!(address, deserialized_address);
        assert_eq!(
            deserialized_address.to_string(),
            "sov1pv9skzctpv9skzctpv9skzctpv9skzctpv9skzctpv9skzctpv9stup8tx"
        );
    }

    #[test]
    /// Enforces that we reject the original (less secure) `bech32` encoding for our address type.
    /// Our addresses should use bech32m only.
    fn test_rejects_non_m_bech32_variant() {
        assert!(Address::<Sha256>::from_str(
            "sov1l6n2cku82yfqld30lanm2nfw43n2auc8clw7r5u5m6s7p8jrm4zqklh0qh"
        )
        .is_err());
    }
}
