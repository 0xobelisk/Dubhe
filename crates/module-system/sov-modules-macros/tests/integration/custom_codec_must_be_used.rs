use std::panic::catch_unwind;

use sov_modules_api::{CryptoSpec, ModuleId, ModuleInfo, Spec, StateCheckpoint, StateValue};
use sov_state::{DefaultStorageSpec, StateCodec, StateItemDecoder, StateItemEncoder, ZkStorage};
use sov_test_utils::ZkTestSpec;

type Hasher = <<ZkTestSpec as Spec>::CryptoSpec as CryptoSpec>::Hasher;

#[derive(ModuleInfo)]
struct TestModule<S>
where
    S: Spec,
{
    #[id]
    id: ModuleId,

    #[state(codec_builder = "crate::CustomCodec::new")]
    state_value: StateValue<u32, CustomCodec>,

    #[phantom]
    phantom: std::marker::PhantomData<S>,
}

#[derive(Default, Clone)]
struct CustomCodec;

impl CustomCodec {
    fn new() -> Self {
        Self
    }
}

impl StateCodec for CustomCodec {
    type KeyCodec = Self;
    type ValueCodec = Self;
    fn key_codec(&self) -> &Self::KeyCodec {
        self
    }
    fn value_codec(&self) -> &Self::ValueCodec {
        self
    }
}

impl<V> StateItemEncoder<V> for CustomCodec {
    fn encode(&self, _value: &V) -> Vec<u8> {
        unimplemented!()
    }
}
impl<V> StateItemDecoder<V> for CustomCodec {
    type Error = String;

    fn try_decode(&self, _bytes: &[u8]) -> Result<V, Self::Error> {
        unimplemented!()
    }
}

fn main() {
    let storage: ZkStorage<DefaultStorageSpec<Hasher>> = ZkStorage::new();
    let module: TestModule<ZkTestSpec> = TestModule::default();

    catch_unwind(|| {
        let mut state: StateCheckpoint<ZkTestSpec> = StateCheckpoint::new(storage);
        module.state_value.set(&0u32, &mut state).unwrap();
    })
    .unwrap_err();
}
