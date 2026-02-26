mod layer;
mod shipper;

pub use layer::{BetterStackLayer, BetterStackLayerBuilder};

use std::time::Duration;

/// Build a [`BetterStackLayer`] from an environment variable.
///
/// Returns `None` if the env var is unset or empty, allowing callers to
/// conditionally wire the layer only when a token is available.
pub fn layer_from_env(env_var: &str) -> Option<BetterStackLayer> {
    let token = std::env::var(env_var).ok().filter(|v| !v.is_empty())?;
    Some(BetterStackLayerBuilder::new(token).build())
}

/// Default Better Stack ingestion endpoint.
pub(crate) const DEFAULT_ENDPOINT: &str = "https://in.logs.betterstack.com";
pub(crate) const DEFAULT_CHANNEL_CAPACITY: usize = 8192;
pub(crate) const DEFAULT_BATCH_SIZE: usize = 100;
pub(crate) const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_secs(1);
