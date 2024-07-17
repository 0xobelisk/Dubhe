use sov_modules_api::{CryptoSpec, ModuleId, ModuleInfo, Spec, StateMap, StateValue};
use sov_test_utils::ZkTestSpec;

mod test_module {
    use super::*;

    #[derive(ModuleInfo)]
    pub(crate) struct TestStruct<S: Spec> {
        #[id]
        pub id: ModuleId,

        // Comment
        #[state]
        pub test_state1: StateMap<<S::CryptoSpec as CryptoSpec>::PublicKey, u32>,

        /// Doc comment
        #[state]
        pub test_state2: StateMap<String, String>,

        #[state]
        pub test_state3: StateValue<String>,
    }
}

fn main() {
    let test_struct = <test_module::TestStruct<ZkTestSpec> as std::default::Default>::default();

    let prefix1 = test_struct.test_state1.prefix();

    assert_eq!(
        *prefix1,
        sov_modules_api::ModulePrefix::new_storage(
            // The tests compile inside trybuild.
            "trybuild000::test_module",
            "TestStruct",
            "test_state1"
        )
        .into()
    );

    let prefix2 = test_struct.test_state2.prefix();
    assert_eq!(
        *prefix2,
        sov_modules_api::ModulePrefix::new_storage(
            // The tests compile inside trybuild.
            "trybuild000::test_module",
            "TestStruct",
            "test_state2"
        )
        .into()
    );

    let prefix2 = test_struct.test_state3.prefix();
    assert_eq!(
        *prefix2,
        sov_modules_api::ModulePrefix::new_storage(
            // The tests compile inside trybuild.
            "trybuild000::test_module",
            "TestStruct",
            "test_state3"
        )
        .into()
    );

    use sov_modules_api::digest::Digest;
    let mut hasher = <<ZkTestSpec as Spec>::CryptoSpec as CryptoSpec>::Hasher::new();
    hasher.update("trybuild000::test_module/TestStruct/".as_bytes());
    let hash: [u8; 32] = hasher.finalize().into();

    assert_eq!(&sov_modules_api::ModuleId::from(hash), test_struct.id());
}
