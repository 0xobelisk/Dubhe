use sov_modules_api::{Context, CryptoSpec, Module, ModuleId, ModuleInfo, Spec, StateMap, TxState};
use sov_test_utils::ZkTestSpec;

pub mod first_test_module {
    use super::*;

    #[derive(ModuleInfo)]
    pub(crate) struct FirstTestStruct<S>
    where
        S: Spec,
    {
        #[id]
        pub id: ModuleId,

        #[state]
        pub state_in_first_struct_1: StateMap<<S::CryptoSpec as CryptoSpec>::PublicKey, u32>,

        #[state]
        pub state_in_first_struct_2: StateMap<String, String>,
    }

    impl<S: Spec> Module for FirstTestStruct<S> {
        type Spec = S;

        type Config = ();

        type CallMessage = ();

        type Event = ();

        fn call(
            &self,
            _message: Self::CallMessage,
            _context: &Context<Self::Spec>,
            _state: &mut impl TxState<S>,
        ) -> Result<sov_modules_api::CallResponse, sov_modules_api::Error> {
            todo!()
        }
    }
}

mod second_test_module {
    use sov_modules_api::Module;

    use super::*;

    #[derive(ModuleInfo)]
    pub(crate) struct SecondTestStruct<S: Spec> {
        #[id]
        pub id: ModuleId,

        #[state]
        pub state_in_second_struct_1: StateMap<String, u32>,

        #[module]
        pub module_in_second_struct_1: first_test_module::FirstTestStruct<S>,
    }

    impl<S: Spec> Module for SecondTestStruct<S> {
        type Spec = S;

        type Config = ();

        type CallMessage = ();

        type Event = ();

        fn call(
            &self,
            _message: Self::CallMessage,
            _context: &Context<Self::Spec>,
            _state: &mut impl TxState<S>,
        ) -> Result<sov_modules_api::CallResponse, sov_modules_api::Error> {
            todo!()
        }
    }
}

fn main() {
    let second_test_struct =
        <second_test_module::SecondTestStruct<ZkTestSpec> as std::default::Default>::default();

    let prefix2 = second_test_struct.state_in_second_struct_1.prefix();
    assert_eq!(
        *prefix2,
        sov_modules_api::ModulePrefix::new_storage(
            // The tests compile inside trybuild.
            "trybuild001::second_test_module",
            "SecondTestStruct",
            "state_in_second_struct_1",
        )
        .into()
    );

    let prefix1 = second_test_struct
        .module_in_second_struct_1
        .state_in_first_struct_1
        .prefix();

    assert_eq!(
        *prefix1,
        sov_modules_api::ModulePrefix::new_storage(
            // The tests compile inside trybuild.
            "trybuild001::first_test_module",
            "FirstTestStruct",
            "state_in_first_struct_1"
        )
        .into()
    );

    assert_eq!(
        second_test_struct.dependencies(),
        [second_test_struct.module_in_second_struct_1.id()]
    );
}
