mod layer;
mod shipper;

pub use layer::{BetterStackLayer, BetterStackLayerBuilder};

use std::time::Duration;

/// Build a [`BetterStackLayer`] from environment variables.
///
/// Returns `None` if the token env var is unset or empty, allowing callers to
/// conditionally wire the layer only when a token is available.
///
/// If `BETTERSTACK_ENDPOINT` is set, it overrides the default ingestion URL.
pub fn layer_from_env(token_env_var: &str) -> Option<BetterStackLayer> {
    let token = std::env::var(token_env_var).ok().filter(|v| !v.is_empty())?;
    let mut builder = BetterStackLayerBuilder::new(token);
    if let Some(endpoint) = std::env::var("BETTERSTACK_ENDPOINT").ok().filter(|v| !v.is_empty()) {
        builder = builder.endpoint(endpoint);
    }
    Some(builder.build())
}

/// Default Better Stack ingestion endpoint.
pub(crate) const DEFAULT_ENDPOINT: &str = "https://in.logs.betterstack.com";
pub(crate) const DEFAULT_CHANNEL_CAPACITY: usize = 8192;
pub(crate) const DEFAULT_BATCH_SIZE: usize = 100;
pub(crate) const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_secs(1);
