//! The ONLY data-egress slice. Sends the combined batch to `POST /v1/agent/batch` with the
//! device bearer token, then PUTs screenshot bytes S3-direct to the ack's presigned URLs.
//! Backoff + jitter and resumable multipart live here (skeleton omits them).
//!
//! Skeleton returns a stub ack so the flow is exercised end-to-end offline. Production: `reqwest`
//! (rustls), `Authorization: Bearer <device jwt>`, `Idempotency-Key: <agent_id>:<batch_seq>`.

use agent_shared::contract::{BatchAck, BatchEnvelope};

pub struct Uploader {
    endpoint: String,
    #[allow(dead_code)]
    token: Option<String>,
}

impl Uploader {
    pub fn from_env() -> Self {
        Uploader {
            endpoint: std::env::var("WP_INGEST_URL")
                .unwrap_or_else(|_| "http://localhost:9000/v1/agent/batch".to_string()),
            token: std::env::var("WP_DEVICE_TOKEN").ok(),
        }
    }

    /// Send one batch and return the server ack (watermark + config version + upload URLs).
    pub async fn send(&self, batch: &BatchEnvelope) -> Result<BatchAck, String> {
        tracing::debug!(endpoint = %self.endpoint, seq = batch.batch_seq, "POST /v1/agent/batch");
        // TODO(reqwest): real HTTPS POST of `batch` with bearer + idempotency header, parse ack.
        // Stub: pretend the server accepted through this seq and config is unchanged.
        Ok(BatchAck {
            watermark_seq: batch.batch_seq,
            config_version: batch.config_version,
            upload_urls: Vec::new(),
        })
    }
}
