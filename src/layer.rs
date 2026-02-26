use std::time::Duration;

use serde_json::Value;
use tokio::sync::mpsc;
use tracing::field::{Field, Visit};
use tracing::span;
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use crate::shipper::run_shipper;
use crate::{DEFAULT_BATCH_SIZE, DEFAULT_CHANNEL_CAPACITY, DEFAULT_ENDPOINT, DEFAULT_FLUSH_INTERVAL};

/// A [`tracing::Layer`] that ships structured JSON log events to Better Stack.
///
/// Events are sent through a bounded channel to a background tokio task that
/// batches and POSTs them. If the channel is full, events are silently dropped
/// so request handlers are never blocked.
pub struct BetterStackLayer {
    tx: mpsc::Sender<Value>,
    /// Keep handle so the shipper task is cancelled on drop.
    _shutdown: tokio::sync::oneshot::Sender<()>,
}

/// Builder for [`BetterStackLayer`].
pub struct BetterStackLayerBuilder {
    source_token: String,
    endpoint: String,
    channel_capacity: usize,
    batch_size: usize,
    flush_interval: Duration,
}

impl BetterStackLayerBuilder {
    /// Create a new builder with the given Better Stack source token.
    pub fn new(source_token: impl Into<String>) -> Self {
        Self {
            source_token: source_token.into(),
            endpoint: DEFAULT_ENDPOINT.to_string(),
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            batch_size: DEFAULT_BATCH_SIZE,
            flush_interval: DEFAULT_FLUSH_INTERVAL,
        }
    }

    /// Override the Better Stack ingestion endpoint.
    pub fn endpoint(mut self, url: impl Into<String>) -> Self {
        self.endpoint = url.into();
        self
    }

    /// Set the bounded channel capacity (default: 8192).
    pub fn channel_capacity(mut self, cap: usize) -> Self {
        self.channel_capacity = cap;
        self
    }

    /// Set the max events per HTTP batch (default: 100).
    pub fn batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    /// Set the max time between flushes (default: 1s).
    pub fn flush_interval(mut self, interval: Duration) -> Self {
        self.flush_interval = interval;
        self
    }

    /// Build the layer and spawn the background shipper task.
    ///
    /// Requires a running tokio runtime.
    pub fn build(self) -> BetterStackLayer {
        let (tx, rx) = mpsc::channel(self.channel_capacity);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(run_shipper(
            rx,
            shutdown_rx,
            self.source_token,
            self.endpoint,
            self.batch_size,
            self.flush_interval,
        ));

        BetterStackLayer {
            tx,
            _shutdown: shutdown_tx,
        }
    }
}

/// Visitor that collects span fields into a JSON map.
struct JsonVisitor {
    fields: serde_json::Map<String, Value>,
}

impl JsonVisitor {
    fn new() -> Self {
        Self {
            fields: serde_json::Map::new(),
        }
    }
}

impl Visit for JsonVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.fields
            .insert(field.name().to_string(), Value::from(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), Value::from(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), Value::from(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name().to_string(), Value::from(value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_string(), Value::from(format!("{:?}", value)));
    }
}

/// Per-span JSON data stored in the registry.
#[derive(Debug)]
struct SpanData {
    name: String,
    fields: serde_json::Map<String, Value>,
}

impl<S> Layer<S> for BetterStackLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        let mut visitor = JsonVisitor::new();
        attrs.record(&mut visitor);

        let data = SpanData {
            name: attrs.metadata().name().to_string(),
            fields: visitor.fields,
        };

        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(data);
        }
    }

    fn on_record(&self, id: &span::Id, values: &span::Record<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut ext = span.extensions_mut();
            if let Some(data) = ext.get_mut::<SpanData>() {
                let mut visitor = JsonVisitor::new();
                values.record(&mut visitor);
                data.fields.extend(visitor.fields);
            }
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let meta = event.metadata();

        // Collect event fields
        let mut visitor = JsonVisitor::new();
        event.record(&mut visitor);

        let message = visitor
            .fields
            .remove("message")
            .unwrap_or_else(|| Value::String(String::new()));

        // Collect innermost span info
        let span_json = ctx.event_span(event).and_then(|span| {
            let ext = span.extensions();
            ext.get::<SpanData>().map(|data| {
                let mut map = serde_json::Map::new();
                map.insert("name".to_string(), Value::String(data.name.clone()));
                for (k, v) in &data.fields {
                    map.insert(k.clone(), v.clone());
                }
                Value::Object(map)
            })
        });

        let now = chrono_now_iso();

        let mut obj = serde_json::Map::new();
        obj.insert("dt".to_string(), Value::String(now));
        obj.insert(
            "level".to_string(),
            Value::String(meta.level().to_string()),
        );
        obj.insert("message".to_string(), message);
        obj.insert(
            "target".to_string(),
            Value::String(meta.target().to_string()),
        );

        if !visitor.fields.is_empty() {
            obj.insert("fields".to_string(), Value::Object(visitor.fields));
        }

        if let Some(span) = span_json {
            obj.insert("span".to_string(), span);
        }

        // Non-blocking send — drop the event if the channel is full.
        let _ = self.tx.try_send(Value::Object(obj));
    }
}

/// Produce an ISO 8601 timestamp without pulling in the `chrono` crate.
fn chrono_now_iso() -> String {
    // Use std SystemTime → format manually.
    let now = std::time::SystemTime::now();
    let dur = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();

    // Convert epoch seconds to a simple ISO string.
    // We avoid pulling in chrono by doing the math ourselves.
    const SECS_PER_DAY: u64 = 86400;
    let days = secs / SECS_PER_DAY;
    let day_secs = secs % SECS_PER_DAY;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    // Days since epoch to Y-M-D (civil calendar).
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, month, day, hours, minutes, seconds, millis
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(days: u64) -> (i64, u64, u64) {
    // Algorithm from Howard Hinnant's `chrono`-compatible civil calendar code.
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
