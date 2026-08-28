//! `GET /v1/me/appearance` — the theme the user picked in the web app, so the panel matches it.
//!
//! The panel used to keep its own `localStorage` theme with its own toggle, which meant a user who
//! set WorkPulse to dark on the web still got a light panel on their desktop, and had to set it a
//! second time in a second place. The account's stored preference is the one the web app already
//! adopts on every sign-in; this makes the agent a second reader of it rather than a rival source
//! of truth.
//!
//! Same user Cognito JWT as every other panel read. Best-effort by design: any failure leaves the
//! panel on whatever it was showing, because a theme is not worth an error state.

use serde::{Deserialize, Serialize};

/// The account's appearance preference.
///
/// `theme` is `"light"`, `"dark"`, or `"system"` — the last being an explicit choice to follow the
/// OS, not the absence of one. The server defaults to `light` rather than `system` precisely so
/// that distinction survives; resolving `"system"` is the client's job, since only it can see the
/// OS setting.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppearanceDto {
    pub theme: String,
    #[serde(default)]
    pub palette: String,
}

pub async fn fetch_appearance(
    client: &reqwest::Client,
    ingest_url: &str,
    id_token: &str,
) -> Result<AppearanceDto, String> {
    let url = format!("{}/v1/me/appearance", ingest_url.trim_end_matches('/'));
    let resp = client
        .get(url)
        .bearer_auth(id_token)
        .send()
        .await
        .map_err(|e| format!("appearance:network:{e}"))?;
    let status = resp.status();
    if status.as_u16() == 401 {
        return Err("auth:expired".into());
    }
    let text = resp
        .text()
        .await
        .map_err(|e| format!("appearance:read:{e}"))?;
    if !status.is_success() {
        return Err(format!("appearance:status:{}:{text}", status.as_u16()));
    }
    parse_appearance(&text)
}

/// Unwrap `{ "data": { … } }` (the backend `Envelope`), else a bare object.
fn parse_appearance(text: &str) -> Result<AppearanceDto, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("appearance:parse:{e}"))?;
    let inner = value.get("data").cloned().unwrap_or(value);
    serde_json::from_value(inner).map_err(|e| format!("appearance:parse:{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_envelope_and_the_bare_object() {
        let enveloped = r#"{"data":{"theme":"dark","palette":"default","version":3}}"#;
        assert_eq!(parse_appearance(enveloped).unwrap().theme, "dark");
        // The bare shape is accepted too, matching how every other panel read is written — the
        // envelope is the server's habit, not a guarantee this module should depend on.
        let bare = r#"{"theme":"light","palette":"default"}"#;
        assert_eq!(parse_appearance(bare).unwrap().theme, "light");
    }

    /// `palette` is optional here even though the server always sends it: this DTO is a subset of a
    /// response the agent doesn't own, and a missing field must not fail the read.
    #[test]
    fn a_missing_palette_is_not_an_error() {
        let v = parse_appearance(r#"{"data":{"theme":"system"}}"#).unwrap();
        assert_eq!(v.theme, "system");
        assert_eq!(v.palette, "");
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(parse_appearance("not json").is_err());
        assert!(parse_appearance(r#"{"data":{"theme":42}}"#).is_err());
    }
}
