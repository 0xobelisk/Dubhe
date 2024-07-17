use sov_modules_api::{CredentialId, PrivateKey, PublicKey, WorkingSet};
use sov_nonces::Nonces;
use sov_prover_storage_manager::new_orphan_storage;
use sov_test_utils::{TestHasher, TestPrivateKey};

type S = sov_test_utils::TestSpec;

#[test]
fn check_hooks_test() {
    let nonces = Nonces::<S>::default();
    let tmpdir = tempfile::tempdir().unwrap();
    let mut working_set =
        WorkingSet::<S>::new_deprecated(new_orphan_storage(tmpdir.path()).unwrap());

    let priv_key = TestPrivateKey::generate();
    let sender = priv_key.pub_key();
    let sender_credential_id: CredentialId = sender.credential_id::<TestHasher>();

    assert!(nonces
        .check_nonce(&sender_credential_id, 0, &mut working_set)
        .is_ok());

    assert!(nonces
        .check_nonce(&sender_credential_id, 1, &mut working_set)
        .is_err());

    let (mut scratchpad, _, _) = working_set.finalize();
    nonces.mark_tx_attempted(&sender_credential_id, &mut scratchpad);

    let mut working_set = scratchpad.commit().to_working_set_unmetered();

    assert!(nonces
        .check_nonce(&sender_credential_id, 0, &mut working_set)
        .is_err());

    assert!(nonces
        .check_nonce(&sender_credential_id, 1, &mut working_set)
        .is_ok());
}
