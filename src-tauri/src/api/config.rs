//! `GET /v1/agent/config` — the pull half of the config rail (BUILD-PLAN §4, backend `fleet`). The
//! batch ack advertises the server's `config_version`; when it differs from what the agent runs, the
//! agent pulls here **conditionally** (`If-None-Match` with the cached ETag) and applies the result
//! live — cadence, blur, silent, and the app/URL rules — with no restart. A 304 costs nothing.

use std::sync::atomic::Ordering;

use serde::Deserialize;
use serde_json::Value;
use wp_agent_contract::{AgentConfig, Cadence, Category};

use crate::state::AppState;

pub enum ConfigPull {
    /// Server config unchanged (304) — keep the cached one.
    NotModified,
    /// A fresh config to apply, plus its ETag for the next conditional pull. Boxed to keep the enum
    /// small next to `NotModified` (clippy `large_enum_variant` — `AgentConfig` keeps growing).
    Fresh {
        config: Box<AgentConfig>,
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
    let config = Box::new(parse_config(&text)?);
    Ok(ConfigPull::Fresh { config, etag })
}

/// Unwrap `{ "data": <config> }` (the backend `Envelope`) if present, else parse directly.
///
/// **Tolerant of enum values this build doesn't know.** `Cadence` and `Category` have no
/// `#[serde(other)]` in the contract, so one new cadence preset or rule category on the server made
/// every installed agent's parse fail, every cycle, and it went on enforcing whatever it had — after
/// a restart, the contract's defaults: no blur, no exceptions, screenshots every 5 minutes (QA audit
/// 2026-10-08, WP-209). Now only the unknown value is replaced, by the conservative stand-in: an
/// unknown cadence becomes `off` (no screenshots rather than a guessed interval), an unknown rule
/// category becomes `neutral` (§14's own "unknown = neutral"). Everything else — blur, exceptions,
/// the blocked lists — is applied as sent.
fn parse_config(text: &str) -> Result<AgentConfig, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("config:parse:{e}"))?;
    let mut inner = value.get("data").cloned().unwrap_or(value);
    match serde_json::from_value::<AgentConfig>(inner.clone()) {
        Ok(config) => Ok(config),
        Err(strict) => {
            let replaced = tolerate_unknown_enums(&mut inner);
            if replaced == 0 {
                return Err(format!("config:parse:{strict}"));
            }
            tracing::warn!(
                replaced,
                "config carries values this agent doesn't know; applied with safe stand-ins ({strict})"
            );
            serde_json::from_value(inner).map_err(|e| format!("config:parse:{e}"))
        }
    }
}

/// Replace every cadence or rule category this build can't deserialize with its conservative
/// stand-in (see [`parse_config`]); returns how many were replaced. "Known" is decided by the
/// contract's own types, so a variant added to the contract is never mistaken for an unknown one.
fn tolerate_unknown_enums(config: &mut Value) -> usize {
    let mut replaced = 0;
    if let Some(cadence) = config.pointer_mut("/tracking/cadence") {
        if serde_json::from_value::<Cadence>(cadence.clone()).is_err() {
            *cadence = Value::from("off");
            replaced += 1;
        }
    }
    for list in ["/rules/apps", "/rules/urls"] {
        let Some(rules) = config.pointer_mut(list).and_then(Value::as_array_mut) else {
            continue;
        };
        for category in rules.iter_mut().filter_map(|r| r.get_mut("category")) {
            if serde_json::from_value::<Category>(category.clone()).is_err() {
                *category = Value::from("neutral");
                replaced += 1;
            }
        }
    }
    replaced
}

// ── the last applied config, kept across restarts ───────────────────────────────────────────────
//
// The applied config lived only in memory, so every launch started from `TrackingConfig::default()`
// — `Min5`, no blur, empty rules: no exceptions, no blocked list — and stayed there until a pull
// succeeded. A config endpoint erroring after a restart meant unblurred screenshots of an org's
// excepted apps every five minutes (WP-209). Now the last applied config is written beside the
// session file and restored at startup, and capture waits until one of the two has happened
// (`AppState::config_known`).

#[derive(Deserialize)]
struct Persisted {
    config: AgentConfig,
    #[serde(default)]
    etag: Option<String>,
}

fn persisted_path() -> std::path::PathBuf {
    crate::outbox::state_dir().join("config.json")
}

fn persist(config: &AgentConfig, etag: Option<&str>) {
    let body = serde_json::json!({ "config": config, "etag": etag });
    if let Err(e) = crate::outbox::write_atomic(&persisted_path(), body.to_string().as_bytes()) {
        tracing::warn!("could not persist the applied config ({e}); a restart will re-pull it");
    }
}

/// Apply a freshly pulled config — live (no restart) and on disk for the next launch.
pub fn apply(state: &AppState, config: AgentConfig, etag: Option<String>) {
    persist(&config, etag.as_deref());
    state.config.lock().unwrap().apply(config, etag);
    state.config_known.store(true, Ordering::Relaxed);
}

/// At startup: re-apply the config the previous run had, if one was saved and still parses.
pub fn restore(state: &AppState) {
    let saved = std::fs::read(persisted_path())
        .ok()
        .and_then(|b| serde_json::from_slice::<Persisted>(&b).ok());
    if let Some(p) = saved {
        tracing::info!(
            version = p.config.version,
            "restored the last applied config"
        );
        state.config.lock().unwrap().apply(p.config, p.etag);
        state.config_known.store(true, Ordering::Relaxed);
    }
}

/// A different user signed in. The ETag is a bare version number, so two organizations can share
/// one, and a pull answering "not modified" would confirm the previous user's policy for the new one.
/// Forget it, and hold capture until this user's config arrives — the next pull is unconditional.
pub fn forget_etag(state: &AppState) {
    state.config_known.store(false, Ordering::Relaxed);
    let mut cache = state.config.lock().unwrap();
    let current = cache.get().clone();
    persist(&current, None);
    cache.apply(current, None);
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// WP-209: a cadence preset or rule category this build doesn't know no longer fails the whole
    /// pull. Only that value falls back — blur and the exceptions still apply.
    #[test]
    fn unknown_enum_values_fall_back_without_losing_the_rest() {
        let future = r#"{"data":{"version":9,
            "tracking":{"version":4,"cadence":"min15","blur_level":3,"retention_days":30,"silent":false},
            "rules":{"version":5,
                "apps":[{"process_name":"code","category":"productive"},
                        {"process_name":"mail","category":"communication"}],
                "urls":[{"domain":"bank.example","category":"sensitive"}],
                "blocked":{"apps":[],"urls":[]},
                "exceptions":{"apps":["1password"],"urls":["bank.example"]}}}}"#;
        let c = parse_config(future).expect("applies despite the unknown values");
        assert_eq!(c.version, 9);
        assert_eq!(
            c.tracking.cadence,
            Cadence::Off,
            "an unknown cadence takes no screenshots"
        );
        assert_eq!(c.tracking.blur_level, 3, "the blur still applies");
        assert_eq!(c.rules.apps[0].category, Category::Productive);
        assert_eq!(c.rules.apps[1].category, Category::Neutral);
        assert_eq!(c.rules.urls[0].category, Category::Neutral);
        assert_eq!(c.rules.exceptions.apps, ["1password"]);
    }

    /// A custom cadence is a known value, not something to replace.
    #[test]
    fn a_custom_cadence_is_kept() {
        let mut v = serde_json::json!({"tracking":{"cadence":{"custom":7}}});
        assert_eq!(tolerate_unknown_enums(&mut v), 0);
        let mut v = serde_json::json!({"tracking":{"cadence":{"hourly":1}}});
        assert_eq!(tolerate_unknown_enums(&mut v), 1);
    }

    /// A malformed body is still an error — tolerance is for unknown values, not garbage.
    #[test]
    fn a_malformed_config_is_still_rejected() {
        assert!(parse_config(r#"{"data":{"version":"nine"}}"#).is_err());
    }
}
