#![no_main]

use std::collections::{HashMap, HashSet};

use libfuzzer_sys::arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::{fuzz_target, Corpus};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{RngCore, SeedableRng};
use sov_accounts::{AccountConfig, AccountData, Accounts, CallMessage};
use sov_modules_api::{
    Context, CredentialId, Module, PrivateKey, PublicKey, Spec, StateCheckpoint,
};
use sov_prover_storage_manager::new_orphan_storage;
use sov_test_utils::{TestHasher, TestPrivateKey};

type S = sov_test_utils::TestSpec;
// Check well-formed calls
fuzz_target!(
    |input: (u16, [u8; 32], [u8; 32], Vec<TestPrivateKey>)| -> Corpus {
        let (iterations, seed, sequencer, keys) = input;
        if iterations < 1024 {
            // pointless to setup & run a small iterations count
            return Corpus::Reject;
        }

        // this is a workaround to the restriction where `ed25519_dalek::Keypair` doesn't implement
        // `Eq` or `Sort`; reduce the set to a unique collection of keys so duplicated accounts are not
        // used.
        let keys = keys
            .into_iter()
            .map(|k| (k.as_hex(), k))
            .collect::<HashMap<_, _>>()
            .into_values()
            .collect::<Vec<_>>();

        if keys.is_empty() {
            return Corpus::Reject;
        }

        let rng = &mut StdRng::from_seed(seed);
        let mut seed = [0u8; 32];
        let tmpdir = tempfile::tempdir().unwrap();
        let storage = new_orphan_storage(tmpdir.path()).unwrap();
        let state = StateCheckpoint::<S>::new(storage);

        let sequencer = <S as Spec>::Address::from(sequencer);
        let accounts: Vec<_> = keys
            .iter()
            .map(|k| AccountData {
                credential_id: k.pub_key().credential_id::<TestHasher>(),
                address: k.to_address(),
            })
            .collect();

        let config = AccountConfig { accounts };

        let accounts: Accounts<S> = Accounts::default();
        let mut genesis_state = state.to_genesis_state_accessor::<Accounts<S>>(&config);
        accounts.genesis(&config, &mut genesis_state).unwrap();

        let mut working_set = genesis_state.checkpoint().to_working_set_unmetered();

        // address list is constant for this test
        let mut used = keys.iter().map(|k| k.as_hex()).collect::<HashSet<_>>();
        let mut state: HashMap<_, _> = keys
            .into_iter()
            .map(|k| (k.to_address::<<S as Spec>::Address>(), k))
            .collect();
        let addresses: Vec<_> = state.keys().copied().collect();

        for i in 0..iterations {
            // we use slices for better select performance
            let sender = addresses.choose(rng).unwrap();
            let context = Context::<S>::new(*sender, Default::default(), sequencer, i as u64);

            // clear previous state
            let previous = state.get(sender).unwrap().as_hex();
            used.remove(&previous);

            // generate an unused key
            rng.fill_bytes(&mut seed);
            let u = &mut Unstructured::new(&seed);
            let mut secret = TestPrivateKey::arbitrary(u).unwrap();
            while used.contains(&secret.as_hex()) {
                rng.fill_bytes(&mut seed);
                let u = &mut Unstructured::new(&seed);
                secret = TestPrivateKey::arbitrary(u).unwrap();
            }
            used.insert(secret.as_hex());

            let public = secret.pub_key();
            state.insert(*sender, secret);

            let credential_id: CredentialId = public.credential_id::<TestHasher>();

            let msg = CallMessage::InsertCredentialId(credential_id);
            accounts.call(msg, &context, &mut working_set).unwrap();
        }

        Corpus::Keep
    }
);
