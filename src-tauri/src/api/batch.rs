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

    /// **DTO field-name test** (BUILD-PLAN §M6). Pins the EXACT JSON keys the batch serializes to —
    /// i.e. the keys the *deployed* ingest deserializes. The live backend binary was compiled against
    /// these names, so a rename/add/remove in `wp-agent-contract` would make it silently reject or
    /// drop fields; this test forces any such change to be conscious (and coordinated with a deploy).
    #[test]
    fn batch_wire_field_names_are_pinned() {
        use wp_agent_contract::{
            ActivityRollup, AgentEvent, AppSpan, Category, Heartbeat, ScreenshotMeta, StopReason,
        };

        let env = BatchEnvelope {
            agent_id: "a".into(),
            batch_seq: 1,
            captured_at: 1,
            config_version: 1,
            heartbeat: Heartbeat {
                hostname: "h".into(),
                os: "windows".into(),
                os_version: "11".into(),
                agent_version: "0".into(),
                ip: "1".into(),
                cpu_pct: 0.0,
                mem_pct: 0.0,
                outbox_mb: 0.0,
                idle: false,
                location: None,
            },
            activity: vec![ActivityRollup {
                minute: 1,
                keystrokes: 1,
                mouse: 1,
                active_sec: 1,
                idle_sec: 1,
                // title/url are `skip_serializing_if none` — set them so the keys are pinned too.
                top_apps: vec![AppSpan {
                    app: "x".into(),
                    title: Some("t".into()),
                    url: Some("u".into()),
                    category: Category::Productive,
                    seconds: 1,
                }],
            }],
            events: vec![AgentEvent::TimerStopped {
                session_id: "s".into(),
                started_at: 0,
                ts: 1,
                reason: StopReason::Idle,
            }],
            screenshots: vec![ScreenshotMeta {
                id: "i".into(),
                captured_at: 1,
                app: "x".into(),
                phash: "p".into(),
                blur_level: 0,
                bucket_minute: 1,
                display: 0,
                content_sha256: String::new(),
            }],
        };

        let v: serde_json::Value = serde_json::to_value(&env).unwrap();
        let keys = |val: &serde_json::Value| -> Vec<String> {
            let mut k: Vec<String> = val.as_object().unwrap().keys().cloned().collect();
            k.sort();
            k
        };

        assert_eq!(
            keys(&v),
            [
                "activity",
                "agent_id",
                "batch_seq",
                "captured_at",
                "config_version",
                "events",
                "heartbeat",
                "screenshots"
            ]
        );
        assert_eq!(
            keys(&v["heartbeat"]),
            [
                "agent_version",
                "cpu_pct",
                "hostname",
                "idle",
                "ip",
                "mem_pct",
                "os",
                "os_version",
                "outbox_mb"
            ]
        );
        assert_eq!(
            keys(&v["activity"][0]),
            [
                "active_sec",
                "idle_sec",
                "keystrokes",
                "minute",
                "mouse",
                "top_apps"
            ]
        );
        assert_eq!(
            keys(&v["activity"][0]["top_apps"][0]),
            ["app", "category", "seconds", "title", "url"]
        );
        assert_eq!(
            keys(&v["screenshots"][0]),
            [
                "app",
                "blur_level",
                "bucket_minute",
                "captured_at",
                "display",
                "id",
                "phash"
            ]
        );
        // Events are internally tagged (`type` + variant fields).
        assert_eq!(
            keys(&v["events"][0]),
            ["reason", "session_id", "started_at", "ts", "type"]
        );

        // Pin the tag/enum string renderings the backend matches on, too.
        assert_eq!(v["events"][0]["type"], "timer_stopped");
        assert_eq!(v["events"][0]["reason"], "idle");
        assert_eq!(v["activity"][0]["top_apps"][0]["category"], "productive");
    }
}
