use clap::Parser;
use sov_celestia_adapter::types::Namespace;
use sov_modules_macros::config_value;

// copy from celestia
const NS_ID_V0_SIZE: usize = 10;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Args {
    #[arg(long, default_value = "http://127.0.0.1:12345")]
    pub(crate) rpc_url: String,

    #[arg(long, default_value = "http://127.0.0.1:12346")]
    pub(crate) rest_url: String,

    /// How many transactions maximum should fit in the batch
    #[arg(long)]
    pub(crate) max_batch_size_tx: u64,

    /// How big batch can be maximum in bytes.
    /// Together with `max_batch_size_tx`
    #[arg(long)]
    pub(crate) max_batch_size_bytes: u64,

    /// To bootstrap account pool.
    #[arg(long)]
    pub(crate) private_keys_dir: String,

    /// Path to genesis folder.
    /// So modules can spin up logic.
    #[arg(long)]
    pub(crate) genesis_dir: String,

    /// Path to rollup_config.toml.
    /// Used to get RPC URL and Celestia endpoint.
    #[arg(long)]
    pub(crate) rollup_config_path: String,

    /// Celestia namespace where batches are processed.
    /// ASCII string max 10 bytes long.
    #[arg(long, default_value = "sov-test")]
    pub(crate) celestia_batch_namespace: String,

    /// Chain ID of the rollup
    #[arg(long, default_value_t = config_value!("CHAIN_ID"))]
    pub(crate) chain_id: u64,

    /// The priority fee expressed as percent, where 1 is 1% 100 is 100%
    #[arg(long, default_value = "10")]
    pub(crate) priority_fee_percent: u64,

    /// How many new users to generate.
    #[arg(long)]
    pub(crate) new_users_count: u64,

    /// How many txs to send. Omit to send messages continously.
    #[arg(long)]
    pub(crate) max_num_txs: Option<usize>,

    /// How frequently (in milliseconds) to pass a message to the message sender. Omit to go as fast as possible.
    #[arg(long)]
    pub(crate) interval: Option<u64>,
}

impl Args {
    pub(crate) fn get_rollup_batch_namespace(&self) -> anyhow::Result<Namespace> {
        if !self.celestia_batch_namespace.is_ascii() {
            anyhow::bail!("--rollup-namespace should be ASCII string");
        }
        if self.celestia_batch_namespace.len() > NS_ID_V0_SIZE {
            anyhow::bail!("--rollup-namespace should be 10 symbols or less");
        }
        // Padded with 0;
        let mut raw_namespace: [u8; 10] = [0; NS_ID_V0_SIZE];

        let arg_bytes = self.celestia_batch_namespace.as_bytes();

        let offset = NS_ID_V0_SIZE - arg_bytes.len();

        for (idx, b) in arg_bytes.iter().enumerate() {
            raw_namespace[idx + offset] = *b;
        }

        Ok(Namespace::const_v0(raw_namespace))
    }
}
