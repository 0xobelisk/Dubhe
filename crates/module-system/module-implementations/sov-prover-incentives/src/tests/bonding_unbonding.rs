use sov_bank::GAS_TOKEN_ID;
use sov_mock_zkvm::MockZkvm;
use sov_test_utils::TEST_DEFAULT_USER_STAKE;

use crate::tests::helpers::setup;
use crate::ProverIncentiveError;

#[test]
/// Tests that the prover can unbond correctly
fn test_unbonding() -> anyhow::Result<()> {
    let (module, prover_address, _, mut state) = setup();
    let token_id = GAS_TOKEN_ID;

    // Get their *unlocked* balance before undbonding
    let initial_unlocked_balance = {
        module
            .bank
            .get_balance_of(&prover_address, token_id, &mut state)?
            .unwrap_or_default()
    };

    let mut working_set = state.to_working_set_unmetered();

    // Unbond the prover
    module.unbond_prover(&prover_address, &mut working_set)?;

    // Assert that the prover no longer has bonded tokens
    assert_eq!(module.get_bond_amount(prover_address, &mut working_set)?, 0);

    // Assert that the prover's unlocked balance has increased by the amount they unbonded
    let unlocked_balance =
        module
            .bank
            .get_balance_of(&prover_address, token_id, &mut working_set)?;
    assert_eq!(
        unlocked_balance,
        Some(TEST_DEFAULT_USER_STAKE + initial_unlocked_balance)
    );

    Ok(())
}

#[test]
/// Tests that the prover cannot submit proofs if unbonded
fn test_prover_not_bonded() -> Result<(), anyhow::Error> {
    let (module, prover_address, _, state) = setup();

    let mut working_set = state.to_working_set_unmetered();

    // Unbond the prover
    module.unbond_prover(&prover_address, &mut working_set)?;

    // Assert that the prover no longer has bonded tokens
    assert_eq!(module.get_bond_amount(prover_address, &mut working_set)?, 0);

    // Process a valid proof
    {
        let proof = &MockZkvm::create_serialized_proof(true, ());
        // Assert that processing a valid proof fails
        assert_eq!(
            module
                .process_proof(proof, &prover_address, &mut working_set)
                .expect_err("The proof should be rejected"),
            ProverIncentiveError::BondNotHighEnough
        );
    }

    Ok(())
}
