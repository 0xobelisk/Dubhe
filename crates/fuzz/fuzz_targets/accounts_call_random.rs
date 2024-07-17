#![no_main]

use libfuzzer_sys::arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use sov_accounts::{Accounts, CallMessage};
use sov_modules_api::{Context, Module, StateCheckpoint};
use sov_prover_storage_manager::new_orphan_storage;

type S = sov_test_utils::TestSpec;

// Check arbitrary, random calls
fuzz_target!(|input: (&[u8], Vec<(Context<S>, CallMessage)>)| {
    let tmpdir = tempfile::tempdir().unwrap();
    let storage = new_orphan_storage(tmpdir.path()).unwrap();
    let state = StateCheckpoint::new(storage);

    let (seed, msgs) = input;
    let u = &mut Unstructured::new(seed);
    let (maybe_accounts, state) = Accounts::arbitrary_workset(u, state);

    let accounts: Accounts<S> = maybe_accounts.unwrap();
    let mut working_set = state.to_working_set_unmetered();

    for (ctx, msg) in msgs {
        // assert malformed calls won't panic
        accounts.call(msg, &ctx, &mut working_set).ok();
    }
});
