use std::env;
use std::str::FromStr;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

pub(crate) fn initialize_logging() {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::from_str(&env::var("RUST_LOG").unwrap_or_else(|_| {
                "info,hyper=info,risc0_zkvm=warn,jmt=info,sov_celestia_adapter=info".to_string()
            }))
            .expect("could not initialize logging"),
        )
        .init();
}
