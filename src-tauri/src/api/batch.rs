//! `POST /v1/agent/batch` — the one ingest call, device/user-authenticated with the Cognito **ID
//! token** (BUILD-PLAN §4). A 401 maps to `auth:expired`. The ack shape is wrapped by the backend's
//! response `Envelope`; we unwrap a `data` field if present, else parse the body directly (the exact
//! wrapper is a **verify-against-live** assumption).

use wp_agent_contract::{BatchAck, BatchEnvelope};

pub async fn send_batch(
    client: &reqwest::Client,
    ingest_url: &str,
    id_token: &str,
    batch: &BatchEnvelope,
) -> Result<BatchAck, String> {
    let url = format!("{}/v1/agent/batch", ingest_url.trim_end_matches('/'));
    let resp = client
        .post(url)
        .bearer_auth(id_token)
        .json(batch)
        .send()
        .await
        .map_err(|e| format!("api:network:{e}"))?;

    let status = resp.status();
    if status.as_u16() == 401 {
        return Err("auth:expired".into());
    }
    let text = resp.text().await.map_err(|e| format!("api:read:{e}"))?;
    if !status.is_success() {
        return Err(format!("api:status:{}:{text}", status.as_u16()));
    }
    parse_ack(&text)
}

/// Unwrap `{ "data": <ack> }` if the backend `Envelope` wraps it, else parse the ack directly.
fn parse_ack(text: &str) -> Result<BatchAck, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("api:parse:{e}"))?;
    let inner = value.get("data").cloned().unwrap_or(value);
    serde_json::from_value(inner).map_err(|e| format!("api:parse:{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_and_enveloped_acks() {
        let bare = r#"{"watermark_seq":5,"config_version":2,"upload_urls":[]}"#;
        assert_eq!(parse_ack(bare).unwrap().watermark_seq, 5);

        let enveloped = r#"{"data":{"watermark_seq":9,"config_version":3}}"#;
        let ack = parse_ack(enveloped).unwrap();
        assert_eq!(ack.watermark_seq, 9);
        assert_eq!(ack.config_version, 3);
    }
}
