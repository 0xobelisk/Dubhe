use std::time::Duration;

/// The biggest blob in bytes we can submit to celestia
/// <https://celestiaorg.github.io/celestia-app/specs/params.html>
/// Our version is a little bit older, so this value differs.
/// Empirically taken from an error message.
// TODO: Will be used when blob size is going to be implemented.
#[allow(dead_code)]
pub(crate) const CELESTIA_MAX_TX_BYTES: u64 = 1973430;

pub(crate) const DEFAULT_MAX_FEE: u64 = 10_000_000;
pub(crate) const DEFAULT_CHANNEL_SIZE: usize = 10_000;
pub(crate) const TIME_OUT_DURATION: Duration = Duration::from_secs(30);
