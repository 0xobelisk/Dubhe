// TODO: Rename this file to change the name of this method from METHOD_NAME

#![no_main]

use const_rollup_config::{ROLLUP_BATCH_NAMESPACE_RAW, ROLLUP_PROOF_NAMESPACE_RAW};
use demo_stf::runtime::Runtime;
use demo_stf::StfVerifier;
use sov_celestia_adapter::types::Namespace;
use sov_celestia_adapter::verifier::CelestiaVerifier;
use sov_kernels::basic::BasicKernel;
use sov_mock_zkvm::{MockZkGuest, MockZkVerifier};
use sov_modules_api::default_spec::DefaultSpec;
use sov_modules_api::execution_mode::Zk;
use sov_modules_stf_blueprint::StfBlueprint;
use sov_risc0_adapter::guest::Risc0Guest;
use sov_risc0_adapter::Risc0Verifier;
use sov_state::ZkStorage;

// The rollup stores its data in the namespace b"sov-test" on Celestia
const ROLLUP_BATCH_NAMESPACE: Namespace = Namespace::const_v0(ROLLUP_BATCH_NAMESPACE_RAW);
const ROLLUP_PROOF_NAMESPACE: Namespace = Namespace::const_v0(ROLLUP_PROOF_NAMESPACE_RAW);

risc0_zkvm::guest::entry!(main);

pub fn main() {
    let guest = Risc0Guest::new();
    let storage = ZkStorage::new();
    let stf: StfBlueprint<
        DefaultSpec<Risc0Verifier, MockZkVerifier, Zk>,
        _,
        Runtime<_, _>,
        BasicKernel<_, _>,
    > = StfBlueprint::new();

    let stf_verifier = StfVerifier::<_, _, _, _, Risc0Guest, MockZkGuest>::new(
        stf,
        CelestiaVerifier {
            rollup_batch_namespace: ROLLUP_BATCH_NAMESPACE,
            rollup_proof_namespace: ROLLUP_PROOF_NAMESPACE,
        },
    );
    stf_verifier
        .run_block(guest, storage)
        .expect("Prover must be honest");
}
