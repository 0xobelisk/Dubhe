use sov_modules_api::cli::JsonStringArg;
use sov_modules_api::macros::{CliWallet, CliWalletArg};
use sov_modules_api::{
    CallResponse, Context, DispatchCall, Error, Genesis, MessageCodec, Module, ModuleId,
    ModuleInfo, Spec, StateValue, TxState,
};
use sov_test_utils::TestSpec;

pub mod first_test_module {
    use super::*;

    #[derive(
        CliWalletArg,
        Debug,
        PartialEq,
        borsh::BorshDeserialize,
        borsh::BorshSerialize,
        serde::Serialize,
        serde::Deserialize,
    )]
    pub struct MyStruct {
        pub first_field: u32,
        pub str_field: String,
    }

    #[derive(ModuleInfo)]
    pub struct FirstTestStruct<S: Spec> {
        #[id]
        pub id: ModuleId,

        #[state]
        pub state_in_first_struct: StateValue<u8>,

        #[phantom]
        phantom: std::marker::PhantomData<S>,
    }

    impl<S: Spec> Module for FirstTestStruct<S> {
        type Spec = S;
        type Config = ();
        type CallMessage = MyStruct;
        type Event = ();

        fn genesis(
            &self,
            _config: &Self::Config,
            _state: &mut impl sov_modules_api::GenesisState<S>,
        ) -> Result<(), Error> {
            Ok(())
        }

        fn call(
            &self,
            _msg: Self::CallMessage,
            _context: &Context<Self::Spec>,
            _state: &mut impl TxState<S>,
        ) -> Result<CallResponse, Error> {
            Ok(CallResponse::default())
        }
    }
}

pub mod second_test_module {
    use super::*;

    #[derive(ModuleInfo)]
    pub struct SecondTestStruct<S: Spec> {
        #[id]
        pub id: ModuleId,

        #[state]
        pub state_in_second_struct: StateValue<u8>,

        #[phantom]
        phantom: std::marker::PhantomData<S>,
    }

    #[derive(
        CliWalletArg,
        Debug,
        PartialEq,
        borsh::BorshDeserialize,
        borsh::BorshSerialize,
        serde::Serialize,
        serde::Deserialize,
    )]
    pub enum MyEnum {
        Foo { first_field: u32, str_field: String },
        Bar(u8),
    }

    impl<S: Spec> Module for SecondTestStruct<S> {
        type Spec = S;
        type Config = ();
        type CallMessage = MyEnum;
        type Event = ();

        fn genesis(
            &self,
            _config: &Self::Config,
            _state: &mut impl sov_modules_api::GenesisState<S>,
        ) -> Result<(), Error> {
            Ok(())
        }

        fn call(
            &self,
            _msg: Self::CallMessage,
            _context: &Context<Self::Spec>,
            _state: &mut impl TxState<S>,
        ) -> Result<CallResponse, Error> {
            Ok(CallResponse::default())
        }
    }
}

#[derive(Default, Genesis, DispatchCall, MessageCodec, CliWallet)]
#[serialization(borsh::BorshDeserialize, borsh::BorshSerialize)]
pub struct Runtime<S: Spec> {
    pub first: first_test_module::FirstTestStruct<S>,
    pub second: second_test_module::SecondTestStruct<S>,
}

fn main() {
    use sov_modules_api::prelude::clap::Parser;

    let expected_foo = RuntimeCall::first(first_test_module::MyStruct {
        first_field: 1,
        str_field: "hello".to_string(),
    });
    let foo_from_cli: RuntimeSubcommand<JsonStringArg, TestSpec> =
        <RuntimeSubcommand<JsonStringArg, TestSpec>>::try_parse_from(&[
            "main",
            "first",
            "--json",
            r#"{"first_field": 1, "str_field": "hello"}"#,
            "--chain-id",
            "0",
        ])
        .expect("parsing must succeed")
        .into();
    let foo_ir: RuntimeMessage<JsonStringArg, TestSpec> = foo_from_cli.try_into().unwrap();
    assert_eq!(expected_foo, foo_ir.try_into().unwrap());

    let expected_bar = RuntimeCall::second(second_test_module::MyEnum::Bar(2));
    let bar_from_cli: RuntimeSubcommand<JsonStringArg, TestSpec> =
        <RuntimeSubcommand<JsonStringArg, TestSpec>>::try_parse_from(&[
            "main",
            "second",
            "--json",
            r#"{"Bar": 2}"#,
            "--chain-id",
            "0",
        ])
        .expect("parsing must succeed")
        .into();
    let bar_ir: RuntimeMessage<JsonStringArg, TestSpec> = bar_from_cli.try_into().unwrap();

    assert_eq!(expected_bar, bar_ir.try_into().unwrap());
}
