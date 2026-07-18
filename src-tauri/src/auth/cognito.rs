//! The three Cognito flows as **hand-rolled JSON POSTs** over `reqwest` (BUILD-PLAN M1) — no SRP, no
//! `aws-sdk`: `InitiateAuth` (USER_PASSWORD_AUTH), `RespondToAuthChallenge` (NEW_PASSWORD_REQUIRED),
//! and `InitiateAuth` (REFRESH_TOKEN_AUTH). Request-body construction is pure + tested; the network
//! round-trip needs a live pool (verify with `owner@acme.test`).

use serde::Deserialize;
use serde_json::{json, Value};

use super::config::CognitoConfig;
use super::token::Tokens;

const CONTENT_TYPE: &str = "application/x-amz-json-1.1";

pub enum LoginOutcome {
    Tokens(Tokens),
    /// An admin-created user signing in for the first time must set a new password.
    NewPasswordRequired {
        session: String,
    },
}

pub fn initiate_auth_body(client_id: &str, username: &str, password: &str) -> Value {
    json!({
        "AuthFlow": "USER_PASSWORD_AUTH",
        "ClientId": client_id,
        "AuthParameters": { "USERNAME": username, "PASSWORD": password }
    })
}

pub fn refresh_body(client_id: &str, refresh_token: &str) -> Value {
    json!({
        "AuthFlow": "REFRESH_TOKEN_AUTH",
        "ClientId": client_id,
        "AuthParameters": { "REFRESH_TOKEN": refresh_token }
    })
}

pub fn new_password_body(
    client_id: &str,
    username: &str,
    new_password: &str,
    session: &str,
) -> Value {
    json!({
        "ChallengeName": "NEW_PASSWORD_REQUIRED",
        "ClientId": client_id,
        "ChallengeResponses": { "USERNAME": username, "NEW_PASSWORD": new_password },
        "Session": session
    })
}

#[derive(Deserialize)]
struct AuthResult {
    #[serde(rename = "IdToken", default)]
    id_token: String,
    #[serde(rename = "AccessToken", default)]
    access_token: String,
    #[serde(rename = "RefreshToken", default)]
    refresh_token: String,
    #[serde(rename = "ExpiresIn", default)]
    expires_in: i64,
}

#[derive(Deserialize)]
struct AuthResponse {
    #[serde(rename = "AuthenticationResult")]
    result: Option<AuthResult>,
    #[serde(rename = "ChallengeName")]
    challenge: Option<String>,
    #[serde(rename = "Session")]
    session: Option<String>,
}

async fn post(
    client: &reqwest::Client,
    cfg: &CognitoConfig,
    target: &str,
    body: Value,
) -> Result<AuthResponse, String> {
    if !cfg.is_configured() {
        return Err("auth:not_configured".into()); // WP_COGNITO_CLIENT_ID unset
    }
    let resp = client
        .post(cfg.idp_endpoint())
        .header("Content-Type", CONTENT_TYPE)
        .header(
            "X-Amz-Target",
            format!("AWSCognitoIdentityProviderService.{target}"),
        )
        .body(serde_json::to_vec(&body).map_err(|e| e.to_string())?)
        .send()
        .await
        .map_err(|e| format!("auth:network:{e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("auth:cognito:{}:{text}", status.as_u16()));
    }
    serde_json::from_str(&text).map_err(|e| format!("auth:parse:{e}"))
}

fn to_tokens(r: AuthResult, prev_refresh: Option<&str>, now_secs: i64) -> Tokens {
    let refresh_token = if r.refresh_token.is_empty() {
        prev_refresh.unwrap_or_default().to_string() // REFRESH_TOKEN_AUTH doesn't return a new one
    } else {
        r.refresh_token
    };
    Tokens {
        id_token: r.id_token,
        access_token: r.access_token,
        refresh_token,
        expires_at: now_secs + r.expires_in.max(0),
    }
}

pub async fn login(
    client: &reqwest::Client,
    cfg: &CognitoConfig,
    username: &str,
    password: &str,
    now_secs: i64,
) -> Result<LoginOutcome, String> {
    let resp = post(
        client,
        cfg,
        "InitiateAuth",
        initiate_auth_body(&cfg.client_id, username, password),
    )
    .await?;
    if let Some(r) = resp.result {
        return Ok(LoginOutcome::Tokens(to_tokens(r, None, now_secs)));
    }
    if resp.challenge.as_deref() == Some("NEW_PASSWORD_REQUIRED") {
        return Ok(LoginOutcome::NewPasswordRequired {
            session: resp.session.unwrap_or_default(),
        });
    }
    Err("auth:unexpected_challenge".into())
}

pub async fn complete_new_password(
    client: &reqwest::Client,
    cfg: &CognitoConfig,
    username: &str,
    new_password: &str,
    session: &str,
    now_secs: i64,
) -> Result<Tokens, String> {
    let resp = post(
        client,
        cfg,
        "RespondToAuthChallenge",
        new_password_body(&cfg.client_id, username, new_password, session),
    )
    .await?;
    let r = resp.result.ok_or("auth:no_result")?;
    Ok(to_tokens(r, None, now_secs))
}

pub async fn refresh(
    client: &reqwest::Client,
    cfg: &CognitoConfig,
    refresh_token: &str,
    now_secs: i64,
) -> Result<Tokens, String> {
    let resp = post(
        client,
        cfg,
        "InitiateAuth",
        refresh_body(&cfg.client_id, refresh_token),
    )
    .await?;
    let r = resp.result.ok_or("auth:no_result")?;
    Ok(to_tokens(r, Some(refresh_token), now_secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initiate_auth_body_shape() {
        let b = initiate_auth_body("client123", "owner@acme.test", "pw");
        assert_eq!(b["AuthFlow"], "USER_PASSWORD_AUTH");
        assert_eq!(b["ClientId"], "client123");
        assert_eq!(b["AuthParameters"]["USERNAME"], "owner@acme.test");
        assert_eq!(b["AuthParameters"]["PASSWORD"], "pw");
    }

    #[test]
    fn refresh_flow_reuses_prev_refresh_token() {
        let r = AuthResult {
            id_token: "id".into(),
            access_token: "ac".into(),
            refresh_token: String::new(), // refresh flow returns none
            expires_in: 3600,
        };
        let t = to_tokens(r, Some("old-refresh"), 1_000);
        assert_eq!(t.refresh_token, "old-refresh");
        assert_eq!(t.expires_at, 4_600);
    }
}
