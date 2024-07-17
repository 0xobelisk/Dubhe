use sov_modules_api::{CryptoSpec, ModuleId, ModuleInfo, Spec, StateMap};

#[derive(ModuleInfo)]
struct FirstTestStruct<S>
where
    S: Spec,
{
    #[id]
    pub id: ModuleId,

    #[state(codec_builder = "sov_state::codec::BorshCodec::default")]
    pub state_in_first_struct_1: StateMap<<S::CryptoSpec as CryptoSpec>::PublicKey, u32>,
}

fn main() {}
