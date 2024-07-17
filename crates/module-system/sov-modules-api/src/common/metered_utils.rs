use std::io;

use borsh::BorshDeserialize;
use digest::consts::U32;
use digest::Digest;
use serde::de::DeserializeOwned;
use sov_modules_macros::config_value;
use sov_rollup_interface::crypto::{SigVerificationError, Signature};
use thiserror::Error;

use crate::{Gas, GasMeter, GasMeteringError};

/// A metered hasher that charges gas for each operation.
/// This data structure should be used in the module system to charge gas when hashing data.
pub struct MeteredHasher<'a, GU: Gas, Meter: GasMeter<GU>, Hasher: Digest<OutputSize = U32>> {
    inner: Hasher,
    meter: &'a mut Meter,
    gas_to_charge_for_hash_update: GU,
    gas_to_charge_for_hash_finalize: GU,
}

impl<'a, GU: Gas, Meter: GasMeter<GU>, Hasher: Digest<OutputSize = U32>>
    MeteredHasher<'a, GU, Meter, Hasher>
{
    /// Default gas price to charge for each hash update operation. This is a per-byte price and it has to be multiplied by the length of the data.
    pub const DEFAULT_GAS_TO_CHARGE_FOR_HASH_UPDATE: [u64; 2] =
        config_value!("GAS_TO_CHARGE_PER_BYTE_HASH_UPDATE");

    /// Default gas price to charge for each hash finalize operation.
    pub const DEFAULT_GAS_TO_CHARGE_FOR_HASH_FINALIZE: [u64; 2] =
        config_value!("GAS_TO_CHARGE_PER_BYTE_HASH_FINALIZE");

    /// Create a new metered hasher from a given gas meter with default gas prices [`Self::DEFAULT_GAS_TO_CHARGE_FOR_HASH_UPDATE`] and [`Self::DEFAULT_GAS_TO_CHARGE_FOR_HASH_FINALIZE`]
    pub fn new(meter: &'a mut Meter) -> Self {
        Self::new_with_custom_price(
            meter,
            GU::from_slice(&Self::DEFAULT_GAS_TO_CHARGE_FOR_HASH_UPDATE),
            GU::from_slice(&Self::DEFAULT_GAS_TO_CHARGE_FOR_HASH_FINALIZE),
        )
    }

    /// Create a new metered hasher from a given gas meter with custom gas prices.
    pub fn new_with_custom_price(
        meter: &'a mut Meter,
        gas_to_charge_for_hash_update: GU,
        gas_to_charge_for_hash_finalize: GU,
    ) -> Self {
        Self {
            inner: Hasher::new(),
            meter,
            gas_to_charge_for_hash_update,
            gas_to_charge_for_hash_finalize,
        }
    }

    /// Update the [`MeteredHasher`] with the given data. Performs the same operation as [`Digest::update`] but charges gas.
    pub fn update(&mut self, data: &[u8]) -> Result<(), GasMeteringError<GU>> {
        self.meter.charge_gas(
            self.gas_to_charge_for_hash_update
                .scalar_product(data.len() as u64),
        )?;
        self.inner.update(data);
        Ok(())
    }

    /// Finalize the [`MeteredHasher`] and return the hash. Performs the same operation as [`Digest::finalize`] but charges gas.
    pub fn finalize(self) -> Result<[u8; 32], (Self, GasMeteringError<GU>)> {
        if let Err(e) = self.meter.charge_gas(&self.gas_to_charge_for_hash_finalize) {
            return Err((self, e));
        };

        let hash = self.inner.finalize();
        Ok(hash.into())
    }

    /// Computes the hash of the given data. Performs the same operation as [`Digest::digest`] but charges gas.
    pub fn digest(data: &[u8], meter: &'a mut Meter) -> Result<[u8; 32], GasMeteringError<GU>> {
        let mut hasher = Self::new(meter);
        hasher.update(data)?;
        Self::finalize(hasher).map_err(|(_, e)| e)
    }
}

/// Representation of a signature verification error.
#[derive(Debug, thiserror::Error)]
pub enum MeteredSigVerificationError<GU: Gas> {
    /// The signature is invalid for the provided public key.
    #[error("Signature verification error: {0}")]
    BadSignature(SigVerificationError),

    /// There is not enough gas to verify the signature.
    #[error("A gas error was raised when trying to verify the signature, {0}")]
    GasError(GasMeteringError<GU>),
}

/// A metered signature that charges gas for signature verification. This is a wrapper around a [`Signature`] struct.
#[derive(
    Debug,
    PartialEq,
    Eq,
    Clone,
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(bound = "GU: serde::Serialize + DeserializeOwned")]
pub struct MeteredSignature<GU: Gas, Sign: Signature> {
    inner: Sign,
    gas_to_charge_per_byte_for_verification: GU,
    fixed_gas_to_charge_per_verification: GU,
}

impl<GU: Gas, Sign: Signature> MeteredSignature<GU, Sign> {
    const DEFAULT_GAS_TO_CHARGE_PER_BYTE_SIGNATURE_VERIFICATION: [u64; 2] =
        config_value!("DEFAULT_GAS_TO_CHARGE_PER_BYTE_SIGNATURE_VERIFICATION");

    const DEFAULT_FIXED_GAS_TO_CHARGE_PER_SIGNATURE_VERIFICATION: [u64; 2] =
        config_value!("DEFAULT_FIXED_GAS_TO_CHARGE_PER_SIGNATURE_VERIFICATION");

    /// Creates a new [`MeteredSignature`] from a given [`Signature`] with a default gas price.
    pub fn new(inner: Sign) -> Self {
        Self {
            inner,
            gas_to_charge_per_byte_for_verification: GU::from_slice(
                &Self::DEFAULT_GAS_TO_CHARGE_PER_BYTE_SIGNATURE_VERIFICATION,
            ),
            fixed_gas_to_charge_per_verification: GU::from_slice(
                &Self::DEFAULT_FIXED_GAS_TO_CHARGE_PER_SIGNATURE_VERIFICATION,
            ),
        }
    }

    /// Creates a new [`MeteredSignature`] from a given [`Signature`] and a gas price.
    pub fn new_with_price(
        inner: Sign,
        fixed_gas_to_charge_per_signature: GU,
        gas_to_charge_per_byte_for_signature: GU,
    ) -> Self {
        Self {
            inner,
            fixed_gas_to_charge_per_verification: fixed_gas_to_charge_per_signature,
            gas_to_charge_per_byte_for_verification: gas_to_charge_per_byte_for_signature,
        }
    }

    /// Verifies a signature with the provided gas meter. This method is a wrapper around [`Signature::verify`].
    pub fn verify(
        &self,
        pub_key: &Sign::PublicKey,
        msg: &[u8],
        meter: &mut impl GasMeter<GU>,
    ) -> Result<(), MeteredSigVerificationError<GU>> {
        let mut fixed_gas_cost = self.fixed_gas_to_charge_per_verification.clone();
        let total_gas_cost = fixed_gas_cost.combine(
            self.gas_to_charge_per_byte_for_verification
                .clone()
                .scalar_product(msg.len() as u64),
        );

        meter
            .charge_gas(total_gas_cost)
            .map_err(|e| MeteredSigVerificationError::GasError(e))?;

        self.inner
            .verify(pub_key, msg)
            .map_err(|e| MeteredSigVerificationError::BadSignature(e))
    }
}

/// Representation of a metered borsh deserialization error.
#[derive(Debug, Error)]
pub enum MeteredBorshDeserializeError<GU: Gas> {
    /// A gas error was raised when trying to deserialize the data.
    #[error("A gas error was raised when trying to deserialize the data, {0}")]
    GasError(GasMeteringError<GU>),
    /// An io error occurred while deserializing the data.
    #[error("IO error: {0}")]
    IOError(io::Error),
}

/// A wrapper around [`BorshDeserialize`] that charges gas for deserialization.
pub trait MeteredBorshDeserialize<GU: Gas>: BorshDeserialize {
    const DEFAULT_GAS_TO_CHARGE_PER_BYTE_BORSH_DESERIALIZATION: [u64; 2] =
        config_value!("DEFAULT_GAS_TO_CHARGE_PER_BYTE_BORSH_DESERIALIZATION");

    /// Verifies a signature with the provided gas meter. This method is a wrapper around [`Signature::verify`].
    fn deserialize(
        buf: &mut &[u8],
        meter: &mut impl GasMeter<GU>,
    ) -> Result<Self, MeteredBorshDeserializeError<GU>> {
        let deserialization_cost =
            GU::from_slice(&Self::DEFAULT_GAS_TO_CHARGE_PER_BYTE_BORSH_DESERIALIZATION);

        Self::deserialize_with_custom_cost(buf, meter, deserialization_cost)
    }

    fn deserialize_with_custom_cost(
        buf: &mut &[u8],
        meter: &mut impl GasMeter<GU>,
        deserialization_cost: GU,
    ) -> Result<Self, MeteredBorshDeserializeError<GU>> {
        meter
            .charge_gas(
                deserialization_cost
                    .clone()
                    .scalar_product(buf.len() as u64),
            )
            .map_err(|e| MeteredBorshDeserializeError::GasError(e))?;

        <Self as BorshDeserialize>::deserialize(buf)
            .map_err(|e| MeteredBorshDeserializeError::IOError(e))
    }
}

impl<T: BorshDeserialize, GU: Gas> MeteredBorshDeserialize<GU> for T {}

#[cfg(test)]
mod test {
    use borsh::{BorshDeserialize, BorshSerialize};
    use sha2::Sha256;
    use sov_mock_zkvm::crypto::private_key::Ed25519PrivateKey;
    use sov_mock_zkvm::crypto::Ed25519Signature;
    use sov_mock_zkvm::MockZkVerifier;
    use sov_prover_storage_manager::new_orphan_storage;
    use sov_rollup_interface::crypto::PrivateKey;
    use sov_rollup_interface::execution_mode::Native;

    use crate::common::gas::GasArray;
    use crate::common::metered_utils::{
        MeteredBorshDeserialize, MeteredBorshDeserializeError, MeteredSigVerificationError,
    };
    use crate::default_spec::DefaultSpec;
    use crate::{Gas, GasPrice, GasUnit, MeteredHasher, MeteredSignature, Spec, WorkingSet};
    type S = DefaultSpec<MockZkVerifier, MockZkVerifier, Native>;

    fn create_working_set(
        remaining_funds: u64,
        gas_price: &<<S as Spec>::Gas as Gas>::Price,
    ) -> WorkingSet<S> {
        let tmpdir = tempfile::tempdir().unwrap();
        let storage = new_orphan_storage(tmpdir.path()).unwrap();
        WorkingSet::new_with_gas_meter(storage, remaining_funds, gas_price)
    }

    #[test]
    fn test_metered_hasher_happy_path() {
        let gas_to_charge_for_hash_update = GasUnit::<2>::from_slice(&[5, 5]);
        let gas_to_charge_for_hash_finalize = GasUnit::<2>::from_slice(&[2, 2]);

        let gas_price = GasPrice::<2>::from_slice(&[1, 1]);

        let data = [1_u8; 32];

        let remaining_funds = gas_to_charge_for_hash_update
            .clone()
            .scalar_product(data.len() as u64)
            .value(&gas_price)
            + gas_to_charge_for_hash_finalize.value(&gas_price);

        let mut ws = create_working_set(remaining_funds, &gas_price);

        let mut hasher = MeteredHasher::<_, _, Sha256>::new_with_custom_price(
            &mut ws,
            gas_to_charge_for_hash_update,
            gas_to_charge_for_hash_finalize,
        );

        assert!(
            hasher.update(&data).is_ok(),
            "Hasher should be able to update"
        );
        assert!(
            hasher.finalize().is_ok(),
            "Hasher should be able to finalize"
        );
    }

    #[test]
    fn test_metered_hasher_not_enough_gas_to_finalize() {
        let gas_to_charge_for_hash_update = GasUnit::<2>::from_slice(&[5, 5]);
        let gas_to_charge_for_hash_finalize = GasUnit::<2>::from_slice(&[2, 2]);

        let gas_price = GasPrice::<2>::from_slice(&[1, 1]);

        let data = [1_u8; 32];

        let remaining_funds = gas_to_charge_for_hash_update
            .clone()
            .scalar_product(data.len() as u64)
            .value(&gas_price);

        let mut ws = create_working_set(remaining_funds, &gas_price);

        let mut hasher = MeteredHasher::<_, _, Sha256>::new_with_custom_price(
            &mut ws,
            gas_to_charge_for_hash_update,
            gas_to_charge_for_hash_finalize,
        );

        assert!(
            hasher.update(&data).is_ok(),
            "Hasher should be able to update"
        );
        assert!(
            hasher.finalize().is_err(),
            "Hasher should not be able to finalize because it should not have enough gas"
        );
    }

    #[test]
    fn test_metered_hasher_not_enough_gas_to_update() {
        let gas_to_charge_for_hash_update = GasUnit::<2>::from_slice(&[5, 5]);
        let gas_to_charge_for_hash_finalize = GasUnit::<2>::from_slice(&[2, 2]);

        let gas_price = GasPrice::<2>::from_slice(&[1, 1]);

        let data = [1_u8; 32];

        let remaining_funds = gas_to_charge_for_hash_update
            .clone()
            .scalar_product(data.len() as u64 - 1)
            .value(&gas_price);

        let mut ws = create_working_set(remaining_funds, &gas_price);

        let mut hasher = MeteredHasher::<_, _, Sha256>::new_with_custom_price(
            &mut ws,
            gas_to_charge_for_hash_update,
            gas_to_charge_for_hash_finalize,
        );

        assert!(
            hasher.update(&data).is_err(),
            "Hasher should be not able to update because it should not have enough gas"
        );
    }

    #[test]
    fn test_metered_signature() {
        let gas_to_charge_for_signature = GasUnit::<2>::from_slice(&[5, 5]);
        let mut fixed_cost = GasUnit::<2>::from_slice(&[1000, 1000]);

        let gas_price = GasPrice::<2>::from_slice(&[1, 1]);

        let data = [1_u8; 32];

        let ed25519 = Ed25519PrivateKey::generate();
        let signature = ed25519.sign(&data);

        let metered_signature = MeteredSignature::<_, Ed25519Signature>::new_with_price(
            signature,
            fixed_cost.clone(),
            gas_to_charge_for_signature.clone(),
        );

        let remaining_funds = fixed_cost
            .combine(
                gas_to_charge_for_signature
                    .clone()
                    .scalar_product(data.len() as u64),
            )
            .value(&gas_price);

        let mut ws = create_working_set(remaining_funds, &gas_price);

        assert!(
            metered_signature
                .verify(&ed25519.pub_key(), &data, &mut ws)
                .is_ok(),
            "Signature should be valid and there should be enough gas available in the metered working set"
        );
    }

    #[test]
    fn test_metered_signature_not_enough_gas() {
        let gas_to_charge_for_signature = GasUnit::<2>::from_slice(&[5, 5]);
        let mut fixed_cost = GasUnit::<2>::from_slice(&[1000, 1000]);

        let gas_price = GasPrice::<2>::from_slice(&[1, 1]);

        let data = [1_u8; 32];

        let ed25519 = Ed25519PrivateKey::generate();
        let signature = ed25519.sign(&data);

        let metered_signature = MeteredSignature::<_, Ed25519Signature>::new_with_price(
            signature,
            fixed_cost.clone(),
            gas_to_charge_for_signature.clone(),
        );

        let remaining_funds = fixed_cost
            .combine(
                gas_to_charge_for_signature
                    .clone()
                    .scalar_product(data.len() as u64 - 1),
            )
            .value(&gas_price);

        let mut ws = create_working_set(remaining_funds, &gas_price);

        assert!(
            matches!(
                metered_signature.verify(&ed25519.pub_key(), &data, &mut ws),
                Err(MeteredSigVerificationError::GasError(..))
            ),
            "There should not be enough gas available in the metered working set"
        );
    }

    #[derive(Debug, BorshSerialize, BorshDeserialize, PartialEq, Eq)]
    pub struct BorshTestStruct {
        pub field1: u32,
        pub field2: u32,
    }

    #[test]
    fn test_metered_deserializer() {
        let gas_to_charge_for_deserialization = GasUnit::<2>::from_slice(&[5, 5]);
        let gas_price = GasPrice::<2>::from_slice(&[1, 1]);

        let data = BorshTestStruct {
            field1: 1,
            field2: 2,
        };
        let serialized_data = borsh::to_vec(&data).unwrap();

        let remaining_funds = gas_to_charge_for_deserialization
            .clone()
            .scalar_product(serialized_data.len() as u64)
            .value(&gas_price);

        let mut ws = create_working_set(remaining_funds, &gas_price);

        let deserialized_data =
            <BorshTestStruct as MeteredBorshDeserialize::<GasUnit<2>>>::deserialize_with_custom_cost(
                &mut serialized_data.as_slice(),
                &mut ws,
                gas_to_charge_for_deserialization,
            )
            .expect("Deserialization should succeed because there should be enough gas available in the gas meter");

        assert_eq!(
            deserialized_data, data,
            "The deserialized data should match the original data"
        );
    }

    #[test]
    fn test_metered_deserializer_not_enough_gas() {
        let gas_to_charge_for_deserialization = GasUnit::<2>::from_slice(&[5, 5]);
        let gas_price = GasPrice::<2>::from_slice(&[1, 1]);

        let data = BorshTestStruct {
            field1: 1,
            field2: 2,
        };
        let serialized_data = borsh::to_vec(&data).unwrap();

        let remaining_funds = gas_to_charge_for_deserialization
            .clone()
            .scalar_product(serialized_data.len() as u64 - 1)
            .value(&gas_price);

        let mut ws = create_working_set(remaining_funds, &gas_price);

        let deserialized_err =
            <BorshTestStruct as MeteredBorshDeserialize::<GasUnit<2>>>::deserialize_with_custom_cost(
                &mut serialized_data.as_slice(),
                &mut ws,
                gas_to_charge_for_deserialization,
            )
            .expect_err("Deserialization should fail because there should not be enough gas available in the gas meter");

        assert!(
            matches!(deserialized_err, MeteredBorshDeserializeError::GasError(..)),
            "The deserialized error should be a gas error"
        );
    }

    #[test]
    fn test_metered_deserializer_invalid_data() {
        let gas_to_charge_for_deserialization = GasUnit::<2>::from_slice(&[5, 5]);
        let gas_price = GasPrice::<2>::from_slice(&[1, 1]);

        let data = BorshTestStruct {
            field1: 1,
            field2: 2,
        };
        let serialized_data = borsh::to_vec(&data).unwrap();

        let remaining_funds = gas_to_charge_for_deserialization
            .clone()
            .scalar_product(serialized_data.len() as u64)
            .value(&gas_price);

        let mut ws = create_working_set(remaining_funds, &gas_price);

        let deserialize_err =
            <BorshTestStruct as MeteredBorshDeserialize<GasUnit<2>>>::deserialize_with_custom_cost(
                &mut &serialized_data[1..],
                &mut ws,
                gas_to_charge_for_deserialization,
            )
            .expect_err("Deserialization should fail because the data is invalid");

        assert!(
            matches!(deserialize_err, MeteredBorshDeserializeError::IOError(..)),
            "The deserialized error should be a borsh deserialize error"
        );
    }
}
