use sov_bank::Bank;
use sov_modules_api::{DaSpec, Genesis, Spec};
use sov_prover_incentives::ProverIncentives;
use sov_sequencer_registry::SequencerRegistry;

pub struct MinimalZkGenesisConfig<S: Spec, Da: DaSpec> {
    pub sequencer_registry: <SequencerRegistry<S, Da> as Genesis>::Config,
    pub prover_incentives: <ProverIncentives<S, Da> as Genesis>::Config,
    pub bank: <Bank<S> as Genesis>::Config,
}
