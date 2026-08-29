//! Hosted-UI OAuth (Google sign-in) for the desktop agent — a native **deep-link + PKCE** flow.
//!
//! The agent is a **public** Cognito client (no secret), so it signs in the way a mobile app would:
//! the authorization-code grant with **PKCE**. We open the system browser to Cognito's Hosted UI
//! (which federates to Google); Cognito redirects to our registered custom scheme
//! `workpulse://callback?code=…`, the OS hands that URL to the running app (via the deep-link and
//! single-instance handlers in `lib.rs`), and we exchange the code for tokens. No embedded browser,
//! no client secret, and no localhost server: the verifier never leaves the process, and `state`
//! guards the redirect against CSRF.
//!
//! This module is the pure OAuth math + HTTP; the *orchestration* (open browser, await the deep
//! link, verify state) lives in `AuthManager::login_google`, which owns the one-shot the deep-link
//! handler fulfils.

use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::config::CognitoConfig;
use super::token::Tokens;

const B64URL: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// The custom scheme Cognito redirects the browser to — registered with the OS (installer for real
/// users, runtime for a dev/portable exe) and listed on the Cognito app client's callback URLs.
pub const REDIRECT_URI: &str = "workpulse://callback";

/// High-entropy OS randomness, base64url — used for the PKCE verifier and the CSRF `state`.
pub fn random_b64url(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).expect("OS RNG unavailable");
    B64URL.encode(buf)
}

/// A PKCE pair: a high-entropy verifier and its S256 challenge (RFC 7636).
pub fn pkce() -> (String, String) {
    let verifier = random_b64url(48); // 64 base64url chars, within the spec's 43..128
    let challenge = B64URL.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// The Cognito Hosted-UI authorize URL that federates to Google, with PKCE + `state`.
pub fn authorize_url(cfg: &CognitoConfig, challenge: &str, state: &str) -> Result<String, String> {
    let mut u =
        url::Url::parse(&cfg.hosted_ui_url("/oauth2/authorize")).map_err(|e| e.to_string())?;
    u.query_pairs_mut()
        .append_pair("client_id", &cfg.client_id)
        .append_pair("response_type", "code")
        .append_pair("scope", "openid email profile")
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("identity_provider", "Google")
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state);
    Ok(u.into())
}

/// `(code, state, error)` parsed from a `workpulse://callback?code=…&state=…` redirect URL.
pub fn parse_callback(url: &str) -> (Option<String>, Option<String>, Option<String>) {
    let query = url.split_once('?').map(|(_, q)| q).unwrap_or("");
    let (mut code, mut state, mut error) = (None, None, None);
    for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            "error" => error = Some(v.into_owned()),
            _ => {}
        }
    }
    (code, state, error)
}

#[derive(Deserialize)]
struct TokenResp {
    id_token: String,
    access_token: String,
    refresh_token: String,
    expires_in: i64,
}

/// Exchange the authorization code for tokens at the Hosted-UI token endpoint (PKCE, no secret).
pub async fn exchange(
    http: &reqwest::Client,
    cfg: &CognitoConfig,
    code: &str,
    verifier: &str,
) -> Result<Tokens, String> {
    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", cfg.client_id.as_str()),
        ("code", code),
        ("redirect_uri", REDIRECT_URI),
        ("code_verifier", verifier),
    ];
    let resp = http
        .post(cfg.hosted_ui_url("/oauth2/token"))
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("auth:oauth: token request failed ({e})"))?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("auth:oauth: token exchange rejected ({body})"));
    }
    let t: TokenResp = resp
        .json()
        .await
        .map_err(|e| format!("auth:oauth: unreadable token response ({e})"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok(Tokens {
        id_token: t.id_token,
        access_token: t.access_token,
        refresh_token: t.refresh_token,
        expires_at: now + t.expires_in,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_the_s256_of_the_verifier() {
        let (v, c) = pkce();
        assert!(v.len() >= 43 && v.len() <= 128);
        assert_eq!(c, B64URL.encode(Sha256::digest(v.as_bytes())));
        assert_ne!(pkce().0, pkce().0); // the verifier is the whole security of the flow
    }

    #[test]
    fn parse_callback_pulls_code_state_and_error_from_the_scheme_url() {
        let (code, state, err) = parse_callback("workpulse://callback?code=abc123&state=xyz");
        assert_eq!(code.as_deref(), Some("abc123"));
        assert_eq!(state.as_deref(), Some("xyz"));
        assert!(err.is_none());
        let (_, _, err) = parse_callback("workpulse://callback?error=access_denied&state=xyz");
        assert_eq!(err.as_deref(), Some("access_denied"));
    }

    #[test]
    fn authorize_url_carries_pkce_state_and_the_scheme_redirect() {
        let cfg = CognitoConfig {
            region: "ap-south-1".into(),
            client_id: "cid".into(),
            ingest_url: "https://api".into(),
            hosted_ui_host: "wp-workpulse-dev.auth.ap-south-1.amazoncognito.com".into(),
        };
        let u = authorize_url(&cfg, "chal", "st").unwrap();
        assert!(u.contains("identity_provider=Google"));
        assert!(u.contains("code_challenge=chal"));
        assert!(u.contains("state=st"));
        assert!(u.contains("redirect_uri=workpulse%3A%2F%2Fcallback"));
    }
}
