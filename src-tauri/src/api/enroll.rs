//! `POST /v1/agent/enroll` — one-time per-install device enrollment (backend `fleet`,
//! MQTT-MIGRATION Phase 3). Trades the user's JWT + the stable per-install `agent_id` for this
//! device's X.509 MQTT identity: the IoT Thing name (= MQTT client id), the certificate + private
//! key, the broker endpoint, and the device's three topics.
//!
//! **The private key is returned EXACTLY ONCE** — the server keeps no copy, so the caller must
//! persist the credential (keyring, `mqtt::store_credential`) before dropping it. Losing it means
//! re-enrolling, which is cheap by design (certs are per-install, like `agent_id`).

use serde::{Deserialize, Serialize};

/// Everything the MQTT task needs, exactly as the enroll response returns it. Persisted verbatim
/// (JSON) in the OS keyring beside the tokens.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceCredential {
    /// IoT Thing name — MUST be used as the MQTT client id (the IoT policy is scoped by it).
    pub thing_name: String,
    pub certificate_pem: String,
    pub private_key: String,
    /// ATS broker hostname (no scheme); mutual-TLS on :8883.
    pub iot_endpoint: String,
    /// Backend → agent commands (subscribe, QoS 1).
    pub cmd_topic: String,
    /// Agent → backend small signals: command acks (publish, QoS 1).
    pub evt_topic: String,
    /// Presence: Last Will `{"online":false}`; `{"online":true}` published on connect.
    pub status_topic: String,
}

pub async fn enroll(
    client: &reqwest::Client,
    ingest_url: &str,
    id_token: &str,
    agent_id: &str,
) -> Result<DeviceCredential, String> {
    let url = format!("{}/v1/agent/enroll", ingest_url.trim_end_matches('/'));
    let resp = client
        .post(url)
        .bearer_auth(id_token)
        .json(&serde_json::json!({ "agent_id": agent_id }))
        .send()
        .await
        .map_err(|e| format!("enroll:network:{e}"))?;

    let status = resp.status();
    if status.as_u16() == 401 {
        return Err("auth:expired".into());
    }
    let text = resp.text().await.map_err(|e| format!("enroll:read:{e}"))?;
    if !status.is_success() {
        return Err(format!("enroll:status:{}:{text}", status.as_u16()));
    }
    parse_credential(&text)
}

/// Unwrap `{ "data": <credential> }` (the backend `Envelope`) if present, else parse directly.
fn parse_credential(text: &str) -> Result<DeviceCredential, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("enroll:parse:{e}"))?;
    let inner = value.get("data").cloned().unwrap_or(value);
    serde_json::from_value(inner).map_err(|e| format!("enroll:parse:{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_enveloped_and_bare_credential() {
        let enveloped = r#"{"data":{"thing_name":"wp-a1","certificate_pem":"CERT","private_key":"KEY","iot_endpoint":"xxx-ats.iot.ap-south-1.amazonaws.com","cmd_topic":"wp/dev/t/t1/agent/a1/cmd","evt_topic":"wp/dev/t/t1/agent/a1/evt","status_topic":"wp/dev/t/t1/agent/a1/status"}}"#;
        let c = parse_credential(enveloped).unwrap();
        assert_eq!(c.thing_name, "wp-a1");
        assert_eq!(c.cmd_topic, "wp/dev/t/t1/agent/a1/cmd");

        let bare = r#"{"thing_name":"n","certificate_pem":"c","private_key":"k","iot_endpoint":"e","cmd_topic":"1","evt_topic":"2","status_topic":"3"}"#;
        let c = parse_credential(bare).unwrap();
        assert_eq!(c.status_topic, "3");
    }

    /// The credential round-trips through its own JSON — the exact blob the keyring persists.
    #[test]
    fn credential_json_round_trip() {
        let c = DeviceCredential {
            thing_name: "t".into(),
            certificate_pem: "cert".into(),
            private_key: "key".into(),
            iot_endpoint: "host".into(),
            cmd_topic: "c".into(),
            evt_topic: "e".into(),
            status_topic: "s".into(),
        };
        let json = serde_json::to_vec(&c).unwrap();
        let back: DeviceCredential = serde_json::from_slice(&json).unwrap();
        assert_eq!(back.thing_name, c.thing_name);
        assert_eq!(back.private_key, c.private_key);
        assert_eq!(back.evt_topic, c.evt_topic);
    }
}
