use demo_simple_stf::{ApplySlotResult, CheckHashPreimageStf};
use sov_mock_da::verifier::MockDaSpec;
use sov_mock_da::{MockAddress, MockBlob, MockBlockHeader, MockValidityCond};
use sov_mock_zkvm::MockZkVerifier;
use sov_rollup_interface::da::RelevantBlobIters;
use sov_rollup_interface::stf::StateTransitionFunction;

#[test]
fn test_stf_success() {
    let address = MockAddress::from([1; 32]);

    let stf = &mut CheckHashPreimageStf::<MockValidityCond>::default();
    StateTransitionFunction::<MockZkVerifier, MockZkVerifier, MockDaSpec>::init_chain(stf, (), ());

    let mut batch_blobs = {
        let incorrect_preimage = vec![1; 32];
        let correct_preimage = vec![0; 32];

        [
            MockBlob::new(incorrect_preimage, address, [0; 32]),
            MockBlob::new(correct_preimage, address, [0; 32]),
        ]
    };

    // Pretend we are in native code and progress the blobs to the verified state.
    for blob in &mut batch_blobs {
        blob.advance();
    }

    let mut proof_blobs = {
        [
            MockBlob::new(vec![0; 32], address, [0; 32]),
            MockBlob::new(vec![0; 32], address, [0; 32]),
        ]
    };

    for blob in &mut proof_blobs {
        blob.advance();
    }

    let relevant_blobs = RelevantBlobIters {
        proof_blobs: &mut proof_blobs,
        batch_blobs: &mut batch_blobs,
    };

    let result = StateTransitionFunction::<MockZkVerifier, MockZkVerifier, MockDaSpec>::apply_slot(
        stf,
        &[],
        (),
        (),
        &MockBlockHeader::default(),
        &MockValidityCond::default(),
        relevant_blobs,
    );

    assert_eq!(2, result.batch_receipts.len());

    let receipt = &result.batch_receipts[0];
    assert_eq!(receipt.inner, ApplySlotResult::Failure);

    let receipt = &result.batch_receipts[1];
    assert_eq!(receipt.inner, ApplySlotResult::Success);
}
