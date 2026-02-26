use std::time::Duration;

use serde_json::Value;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

/// Background task that receives log events from the channel, batches them,
/// and POSTs JSON arrays to the Better Stack HTTP ingestion API.
pub(crate) async fn run_shipper(
    mut rx: mpsc::Receiver<Value>,
    mut shutdown: oneshot::Receiver<()>,
    source_token: String,
    endpoint: String,
    batch_size: usize,
    flush_interval: Duration,
) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut batch: Vec<Value> = Vec::with_capacity(batch_size);
    let mut interval = tokio::time::interval(flush_interval);
    // The first tick completes immediately — skip it.
    interval.tick().await;

    loop {
        tokio::select! {
            biased;

            // Shutdown signal — flush remaining and exit.
            _ = &mut shutdown => {
                // Drain anything left in the channel.
                while let Ok(event) = rx.try_recv() {
                    batch.push(event);
                    if batch.len() >= batch_size {
                        flush(&client, &endpoint, &source_token, &mut batch).await;
                    }
                }
                if !batch.is_empty() {
                    flush(&client, &endpoint, &source_token, &mut batch).await;
                }
                return;
            }

            // Receive an event.
            maybe_event = rx.recv() => {
                match maybe_event {
                    Some(event) => {
                        batch.push(event);
                        if batch.len() >= batch_size {
                            flush(&client, &endpoint, &source_token, &mut batch).await;
                        }
                    }
                    // Channel closed — flush and exit.
                    None => {
                        if !batch.is_empty() {
                            flush(&client, &endpoint, &source_token, &mut batch).await;
                        }
                        return;
                    }
                }
            }

            // Timer tick — flush whatever we have.
            _ = interval.tick() => {
                if !batch.is_empty() {
                    flush(&client, &endpoint, &source_token, &mut batch).await;
                }
            }
        }
    }
}

async fn flush(
    client: &reqwest::Client,
    endpoint: &str,
    source_token: &str,
    batch: &mut Vec<Value>,
) {
    let events: Vec<Value> = batch.drain(..).collect();
    match client
        .post(endpoint)
        .bearer_auth(source_token)
        .json(&events)
        .send()
        .await
    {
        Ok(resp) if !resp.status().is_success() => {
            eprintln!(
                "[lasersell-tel] Better Stack returned HTTP {}",
                resp.status()
            );
        }
        Err(err) => {
            eprintln!("[lasersell-tel] failed to ship logs to Better Stack: {err}");
        }
        _ => {}
    }
}
