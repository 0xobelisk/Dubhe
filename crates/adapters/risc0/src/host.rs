//! This module implements the [`ZkvmHost`] trait for the RISC0 VM.

use risc0_zkvm::{ExecutorEnvBuilder, ExecutorImpl, Journal, Receipt, Session};
use sov_rollup_interface::zk::{Proof, ZkvmHost};

use crate::guest::Risc0Guest;

/// A [`Risc0Host`] stores a binary to execute in the Risc0 VM, and accumulates hints to be
/// provided to its execution.
#[derive(Clone)]
pub struct Risc0Host<'a> {
    env: Vec<u32>,
    elf: &'a [u8],
}

#[cfg(not(feature = "bench"))]
#[inline(always)]
fn add_benchmarking_callbacks(env: ExecutorEnvBuilder<'_>) -> ExecutorEnvBuilder<'_> {
    env
}

#[cfg(feature = "bench")]
fn add_benchmarking_callbacks(mut env: ExecutorEnvBuilder<'_>) -> ExecutorEnvBuilder<'_> {
    use risc0_cycle_utils::{cycle_count_callback, get_syscall_name, get_syscall_name_cycles};

    use crate::metrics::metrics_callback;

    let metrics_syscall_name = get_syscall_name();
    env.io_callback(metrics_syscall_name, metrics_callback);

    let cycles_syscall_name = get_syscall_name_cycles();
    env.io_callback(cycles_syscall_name, cycle_count_callback);

    env
}

impl<'a> Risc0Host<'a> {
    /// Create a new Risc0Host to prove the given binary.
    pub fn new(elf: &'a [u8]) -> Self {
        Self {
            env: Default::default(),
            elf,
        }
    }

    /// Run a computation in the zkVM without generating a receipt.
    /// This creates the "Session" trace without invoking the heavy cryptographic machinery.
    pub fn run_without_proving(&mut self) -> anyhow::Result<Session> {
        let env = add_benchmarking_callbacks(ExecutorEnvBuilder::default())
            .write_slice(&self.env)
            .build()
            .unwrap();
        let mut executor = ExecutorImpl::from_elf(env, self.elf)?;
        executor.run()
    }
    /// Run a computation in the zkvm and generate a receipt.
    pub fn run(&mut self) -> anyhow::Result<Receipt> {
        let session = self.run_without_proving()?;
        session.prove()
    }
}

impl<'a> ZkvmHost for Risc0Host<'a> {
    type Guest = Risc0Guest;

    fn add_hint<T: serde::Serialize>(&mut self, item: T) {
        // We use the in-memory size of `item` as an indication of how much
        // space to reserve. This is in no way guaranteed to be exact, but
        // usually the in-memory size and serialized data size are quite close.
        //
        // Note: this is just an optimization to avoid frequent reallocation,
        // it's not actually required.
        self.env
            .reserve(std::mem::size_of::<T>() / std::mem::size_of::<u32>());

        let mut serializer = risc0_zkvm::serde::Serializer::new(&mut self.env);
        item.serialize(&mut serializer)
            .expect("Risc0 hint serialization is infallible");
    }

    fn simulate_with_hints(&mut self) -> Self::Guest {
        Risc0Guest::with_hints(std::mem::take(&mut self.env))
    }

    fn run(&mut self, with_proof: bool) -> Result<Vec<u8>, anyhow::Error> {
        let proof = if with_proof {
            let receipt = self.run()?;
            Proof::<Receipt, Option<Journal>>::Full(receipt)
        } else {
            let session = self.run_without_proving()?;
            let data = session.journal;
            Proof::<Receipt, Option<Journal>>::PublicData(data)
        };

        Ok(bincode::serialize(&proof)?)
    }
}
