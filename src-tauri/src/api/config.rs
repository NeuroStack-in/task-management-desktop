//! `GET /v1/agent/config` — the pull half of the config rail (BUILD-PLAN §4, backend `fleet`). The
//! batch ack advertises the server's `config_version`; when it differs from what the agent runs, the
//! agent pulls here **conditionally** (`If-None-Match` with the cached ETag) and applies the result
//! live — cadence, blur, silent, and the app/URL rules — with no restart. A 304 costs nothing.

use wp_agent_contract::AgentConfig;

pub enum ConfigPull {
    /// Server config unchanged (304) — keep the cached one.
    NotModified,
    /// A fresh config to apply, plus its ETag for the next conditional pull.
    Fresh {
        config: AgentConfig,
        etag: Option<String>,
    },
}

pub async fn pull_config(
    client: &reqwest::Client,
    ingest_url: &str,
    id_token: &str,
    if_none_match: Option<&str>,
) -> Result<ConfigPull, String> {
    let url = format!("{}/v1/agent/config", ingest_url.trim_end_matches('/'));
    let mut req = client.get(url).bearer_auth(id_token);
    if let Some(etag) = if_none_match {
        req = req.header(reqwest::header::IF_NONE_MATCH, etag);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("config:network:{e}"))?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(ConfigPull::NotModified);
    }
    if status.as_u16() == 401 {
        return Err("auth:expired".into());
    }
    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("config:status:{}:{body}", status.as_u16()));
    }
    let text = resp.text().await.map_err(|e| format!("config:read:{e}"))?;
    let config = parse_config(&text)?;
    Ok(ConfigPull::Fresh { config, etag })
}

/// Unwrap `{ "data": <config> }` (the backend `Envelope`) if present, else parse directly.
fn parse_config(text: &str) -> Result<AgentConfig, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("config:parse:{e}"))?;
    let inner = value.get("data").cloned().unwrap_or(value);
    serde_json::from_value(inner).map_err(|e| format!("config:parse:{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_agent_contract::Cadence;

    #[test]
    fn parses_enveloped_and_bare_config() {
        let enveloped = r#"{"data":{"version":7,"tracking":{"version":5,"cadence":"min10","blur_level":2,"retention_days":30,"silent":false,"auto_update":true},"rules":{"version":2,"apps":[],"urls":[],"blocked":{"apps":[],"urls":[]},"exceptions":{"apps":[],"urls":[]}}}}"#;
        let c = parse_config(enveloped).unwrap();
        assert_eq!(c.version, 7);
        assert_eq!(c.tracking.cadence, Cadence::Min10);
        assert_eq!(c.tracking.blur_level, 2);

        let bare = r#"{"version":1,"tracking":{"version":1,"cadence":"off","blur_level":0,"retention_days":90,"silent":true,"auto_update":true},"rules":{"version":0,"apps":[],"urls":[],"blocked":{"apps":[],"urls":[]},"exceptions":{"apps":[],"urls":[]}}}"#;
        let c = parse_config(bare).unwrap();
        assert_eq!(c.version, 1);
        assert!(c.tracking.silent);
        assert_eq!(c.tracking.cadence, Cadence::Off);
    }
}
