//! Hosted-UI OAuth (Google sign-in) for the desktop agent — a native **loopback + PKCE** flow.
//!
//! The agent is a **public** Cognito client (no secret), so it signs in the way a mobile app would:
//! the authorization-code grant with **PKCE**. We open the system browser to Cognito's Hosted UI
//! (which federates to Google), catch the redirect on a `localhost` loopback, and exchange the code
//! for tokens. No embedded browser, no client secret — the code verifier never leaves the process,
//! and `state` guards the redirect against CSRF.
//!
//! `open` is injected (the caller launches the browser) so this module stays free of the Tauri
//! handle; on success it returns the same [`Tokens`] a password login produces.

use std::time::Duration;

use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::config::{CognitoConfig, OAUTH_LOOPBACK_PORT};
use super::token::Tokens;

const B64URL: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;
/// How long to wait on the loopback for the browser redirect before giving up.
const REDIRECT_TIMEOUT: Duration = Duration::from_secs(300);

fn random_b64url(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).expect("OS RNG unavailable");
    B64URL.encode(buf)
}

/// A PKCE pair: a high-entropy verifier and its S256 challenge (RFC 7636).
fn pkce() -> (String, String) {
    let verifier = random_b64url(48); // 64 base64url chars, within the spec's 43..128
    let challenge = B64URL.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// The exact loopback the Hosted UI redirects back to — must be registered on the app client.
fn redirect_uri() -> String {
    format!("http://localhost:{OAUTH_LOOPBACK_PORT}/callback")
}

/// Run the whole flow: build the authorize URL, hand it to `open` (which launches the browser), wait
/// on the loopback for the `?code=`, and exchange it for tokens.
pub async fn login<F>(http: &reqwest::Client, cfg: &CognitoConfig, open: F) -> Result<Tokens, String>
where
    F: FnOnce(&str) -> Result<(), String>,
{
    let (verifier, challenge) = pkce();
    let state = random_b64url(24);

    // Bind the loopback FIRST — if the port is taken, fail before sending someone to a browser whose
    // redirect nothing is listening for.
    let listener = TcpListener::bind(("127.0.0.1", OAUTH_LOOPBACK_PORT))
        .await
        .map_err(|e| {
            format!("auth:oauth: couldn't open local callback port {OAUTH_LOOPBACK_PORT} ({e})")
        })?;

    let mut authorize =
        url::Url::parse(&cfg.hosted_ui_url("/oauth2/authorize")).map_err(|e| e.to_string())?;
    authorize
        .query_pairs_mut()
        .append_pair("client_id", &cfg.client_id)
        .append_pair("response_type", "code")
        .append_pair("scope", "openid email profile")
        .append_pair("redirect_uri", &redirect_uri())
        .append_pair("identity_provider", "Google")
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state);
    open(authorize.as_str())?;

    let code = wait_for_code(&listener, &state).await?;
    exchange(http, cfg, &code, &verifier).await
}

/// Accept one loopback request, verify `state`, answer the browser, and return the `code`.
async fn wait_for_code(listener: &TcpListener, expected_state: &str) -> Result<String, String> {
    let accept = async {
        loop {
            let (mut sock, _) = listener.accept().await.map_err(|e| e.to_string())?;
            let mut buf = [0u8; 4096];
            let n = sock.read(&mut buf).await.map_err(|e| e.to_string())?;
            let req = String::from_utf8_lossy(&buf[..n]);
            // Request line: `GET /callback?code=...&state=... HTTP/1.1`.
            let path = req
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("");
            if !path.starts_with("/callback") {
                // A stray hit (favicon, probe) — 404 and keep waiting for the real redirect.
                let _ = sock
                    .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                    .await;
                continue;
            }
            let (code, st, err) = parse_callback(path);
            let ok = err.is_none() && st.as_deref() == Some(expected_state) && code.is_some();
            reply(&mut sock, ok).await;

            if let Some(e) = err {
                return Err(format!("auth:oauth: the sign-in was declined ({e})"));
            }
            if st.as_deref() != Some(expected_state) {
                return Err("auth:oauth: state mismatch (possible CSRF) — sign-in aborted".into());
            }
            return code.ok_or_else(|| "auth:oauth: no authorization code in the redirect".into());
        }
    };
    match tokio::time::timeout(REDIRECT_TIMEOUT, accept).await {
        Ok(r) => r,
        Err(_) => Err("auth:oauth: timed out waiting for the browser sign-in".into()),
    }
}

/// A tiny HTML page so the browser tab shows a human result instead of a blank/failed load.
async fn reply(sock: &mut tokio::net::TcpStream, ok: bool) {
    let body = if ok {
        "Signed in. You can close this window and return to WorkPulse."
    } else {
        "Sign-in failed. You can close this window and try again."
    };
    let html = format!(
        "<!doctype html><meta charset=utf-8><title>WorkPulse</title>\
         <body style=\"font:16px system-ui;padding:3rem;text-align:center;color:#1e1b4b\">{body}</body>"
    );
    let status = if ok { "200 OK" } else { "400 Bad Request" };
    let resp = format!(
        "HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{html}",
        html.len()
    );
    let _ = sock.write_all(resp.as_bytes()).await;
    let _ = sock.flush().await;
}

/// `(code, state, error)` from a `/callback?…` path.
fn parse_callback(path: &str) -> (Option<String>, Option<String>, Option<String>) {
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
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
async fn exchange(
    http: &reqwest::Client,
    cfg: &CognitoConfig,
    code: &str,
    verifier: &str,
) -> Result<Tokens, String> {
    let redirect = redirect_uri();
    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", cfg.client_id.as_str()),
        ("code", code),
        ("redirect_uri", redirect.as_str()),
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
        // Two calls must not collide — the verifier is the whole security of the flow.
        assert_ne!(pkce().0, pkce().0);
    }

    #[test]
    fn parse_callback_pulls_code_state_and_error() {
        let (code, state, err) = parse_callback("/callback?code=abc123&state=xyz");
        assert_eq!(code.as_deref(), Some("abc123"));
        assert_eq!(state.as_deref(), Some("xyz"));
        assert!(err.is_none());
        let (_, _, err) = parse_callback("/callback?error=access_denied&state=xyz");
        assert_eq!(err.as_deref(), Some("access_denied"));
    }
}
