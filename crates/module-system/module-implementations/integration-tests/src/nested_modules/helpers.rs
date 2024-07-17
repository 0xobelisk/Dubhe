use sov_modules_api::{Context, EventEmitter, ModuleInfo, Spec, StateMap, StateValue, WorkingSet};

#[derive(
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    PartialEq,
    Clone,
)]
pub(crate) enum Event {
    Update,
    Execute,
}

pub mod module_a {
    use sov_modules_api::{Module, ModuleId, TxState};

    use super::*;

    #[derive(ModuleInfo)]
    pub(crate) struct ModuleA<S: Spec> {
        #[id]
        pub id_module_a: ModuleId,

        #[state]
        pub(crate) state_1_a: StateMap<String, String>,

        #[state]
        pub(crate) state_2_a: StateValue<String>,

        #[phantom]
        phantom: std::marker::PhantomData<S>,
    }

    impl<S: Spec> Module for ModuleA<S> {
        type Spec = S;

        type Config = ();

        type CallMessage = ();

        type Event = Event;

        fn call(
            &self,
            _message: Self::CallMessage,
            _context: &Context<Self::Spec>,
            _state: &mut impl TxState<Self::Spec>,
        ) -> Result<sov_modules_api::CallResponse, sov_modules_api::Error> {
            todo!()
        }
    }

    impl<S: Spec> ModuleA<S> {
        pub fn update(
            &mut self,
            key: &str,
            value: &str,
            state: &mut WorkingSet<S>,
        ) -> Result<(), anyhow::Error> {
            self.emit_event(state, Event::Update);
            self.state_1_a
                .set(&key.to_owned(), &value.to_owned(), state)?;
            self.state_2_a.set(&value.to_owned(), state)?;
            Ok(())
        }
    }
}

pub mod module_b {
    use sov_modules_api::{Module, ModuleId, TxState};

    use super::*;

    #[derive(ModuleInfo)]
    pub(crate) struct ModuleB<S: Spec> {
        #[id]
        pub id_module_b: ModuleId,

        #[state]
        state_1_b: StateMap<String, String>,

        #[module]
        pub(crate) mod_1_a: module_a::ModuleA<S>,
    }

    impl<S: Spec> Module for ModuleB<S> {
        type Spec = S;

        type Config = ();

        type CallMessage = ();

        type Event = Event;

        fn call(
            &self,
            _message: Self::CallMessage,
            _context: &Context<Self::Spec>,
            _state: &mut impl TxState<Self::Spec>,
        ) -> Result<sov_modules_api::CallResponse, sov_modules_api::Error> {
            todo!()
        }
    }

    impl<S: Spec> ModuleB<S> {
        pub fn update(
            &mut self,
            key: &str,
            value: &str,
            state: &mut WorkingSet<S>,
        ) -> Result<(), anyhow::Error> {
            self.emit_event(state, Event::Update);
            self.state_1_b
                .set(&key.to_owned(), &value.to_owned(), state)?;
            self.mod_1_a.update("key_from_b", value, state)?;
            Ok(())
        }
    }
}

pub(crate) mod module_c {
    use sov_modules_api::{Module, ModuleId, TxState};

    use super::*;

    #[derive(ModuleInfo)]
    pub(crate) struct ModuleC<S: Spec> {
        #[id]
        pub id: ModuleId,

        #[module]
        pub(crate) mod_1_a: module_a::ModuleA<S>,

        #[module]
        mod_1_b: module_b::ModuleB<S>,
    }

    impl<S: Spec> Module for ModuleC<S> {
        type Spec = S;

        type Config = ();

        type CallMessage = ();

        type Event = Event;

        fn call(
            &self,
            _message: Self::CallMessage,
            _context: &Context<Self::Spec>,
            _state: &mut impl TxState<Self::Spec>,
        ) -> Result<sov_modules_api::CallResponse, sov_modules_api::Error> {
            todo!()
        }
    }

    impl<S: Spec> ModuleC<S> {
        pub fn execute(
            &mut self,
            key: &str,
            value: &str,
            state: &mut WorkingSet<S>,
        ) -> Result<(), anyhow::Error> {
            self.emit_event(state, Event::Execute);
            self.mod_1_a.update(key, value, state)?;
            self.mod_1_b.update(key, value, state)?;
            self.mod_1_a.update(key, value, state)?;
            Ok(())
        }
    }
}
