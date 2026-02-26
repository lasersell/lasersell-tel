# lasersell-tel

Non-blocking [tracing](https://docs.rs/tracing) layer that ships structured JSON logs to [Better Stack](https://betterstack.com).

## How it works

```
tracing event → BetterStackLayer → bounded mpsc channel → background shipper task
                                                              ↓
                                                    batch (100 events or 1s)
                                                              ↓
                                                    POST JSON to Better Stack
```

- **Non-blocking:** Events are sent through a bounded channel. If Better Stack is unreachable and the channel fills up, events are dropped — request handlers are never blocked.
- **Batching:** The background task collects up to 100 events or flushes every 1 second, whichever comes first.
- **Graceful shutdown:** On drop, remaining events are flushed before the background task exits.

## Usage

```rust
use tracing_subscriber::prelude::*;

let env_filter = tracing_subscriber::EnvFilter::from_default_env();
let fmt_layer = tracing_subscriber::fmt::layer();

let registry = tracing_subscriber::registry()
    .with(env_filter)
    .with(fmt_layer);

// Only added when the env var is set — otherwise logs go to stdout only.
if let Some(bs) = lasersell_tel::layer_from_env("BETTERSTACK_SOURCE_TOKEN") {
    registry.with(bs).init();
} else {
    registry.init();
}
```

## Builder API

```rust
use std::time::Duration;
use lasersell_tel::BetterStackLayerBuilder;

let layer = BetterStackLayerBuilder::new("your-source-token")
    .endpoint("https://in.logs.betterstack.com")  // default
    .channel_capacity(8192)                        // default
    .batch_size(100)                               // default
    .flush_interval(Duration::from_secs(1))        // default
    .build();
```

## Log format

Each event is sent as a JSON object:

```json
{
  "dt": "2026-02-26T12:34:56.789Z",
  "level": "INFO",
  "message": "exit_api_response",
  "target": "exit_api",
  "fields": {
    "event": "exit_api_response",
    "endpoint": "/v1/buy",
    "latency_ms": 42
  },
  "span": {
    "name": "request",
    "request_id": "a1b2c3"
  }
}
```

Better Stack automatically parses JSON — all fields become searchable in the dashboard.

## License

MIT
