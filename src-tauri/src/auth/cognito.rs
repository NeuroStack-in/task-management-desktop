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
    /// A second factor is required. `challenge` is echoed back verbatim on the response, because the
    /// answer field differs per challenge (`SOFTWARE_TOKEN_MFA_CODE` vs `SMS_MFA_CODE`).
    ///
    /// Before this existed the whole branch fell through to `auth:unexpected_challenge`, so any user
    /// with TOTP enrolled simply could not sign in to the agent while signing in fine on the web.
    MfaRequired {
        challenge: String,
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
        return Err(cognito_error(status.as_u16(), &text));
    }
    serde_json::from_str(&text).map_err(|e| format!("auth:parse:{e}"))
}

/// Turn a Cognito error body into `auth:cognito:<Type>:<message>`.
///
/// Cognito answers `{"__type":"NotAuthorizedException","message":"…"}`. Both halves matter and both
/// were being discarded into one opaque blob: the **type** is what the UI branches on, and the
/// **message** is Cognito's own words — for an expired temporary password that sentence *is* the
/// diagnosis ("Temporary password has expired and must be reset by an administrator"), and it is
/// indistinguishable from a genuinely wrong password unless it is surfaced.
///
/// `__type` is sometimes namespaced (`com.amazonaws.cognito.identity#NotAuthorizedException`), so
/// only the segment after the last `#` is kept.
fn cognito_error(status: u16, body: &str) -> String {
    #[derive(Deserialize)]
    struct WireError {
        #[serde(rename = "__type", default)]
        kind: String,
        #[serde(default)]
        message: String,
    }
    match serde_json::from_str::<WireError>(body) {
        Ok(e) if !e.kind.is_empty() => {
            let kind = e.kind.rsplit('#').next().unwrap_or(e.kind.as_str());
            if e.message.is_empty() {
                format!("auth:cognito:{kind}")
            } else {
                format!("auth:cognito:{kind}:{}", e.message)
            }
        }
        // Not the shape we expected — keep the raw body rather than inventing a reason for it.
        _ => format!("auth:cognito:{status}:{body}"),
    }
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
    match resp.challenge.as_deref() {
        Some("NEW_PASSWORD_REQUIRED") => Ok(LoginOutcome::NewPasswordRequired {
            session: resp.session.unwrap_or_default(),
        }),
        Some(c @ ("SOFTWARE_TOKEN_MFA" | "SMS_MFA")) => Ok(LoginOutcome::MfaRequired {
            challenge: c.to_string(),
            session: resp.session.unwrap_or_default(),
        }),
        // Name the challenge we can't answer. `auth:unexpected_challenge` alone sent people looking
        // at their password for what is actually an unsupported flow (e.g. `MFA_SETUP`, which needs
        // an enrolment round-trip the agent does not implement).
        Some(other) => Err(format!("auth:unsupported_challenge:{other}")),
        None => Err("auth:no_result".into()),
    }
}

/// Answer a `SOFTWARE_TOKEN_MFA` / `SMS_MFA` challenge with the user's 6-digit code.
pub async fn respond_mfa(
    client: &reqwest::Client,
    cfg: &CognitoConfig,
    challenge: &str,
    username: &str,
    code: &str,
    session: &str,
    now_secs: i64,
) -> Result<Tokens, String> {
    let resp = post(
        client,
        cfg,
        "RespondToAuthChallenge",
        mfa_body(&cfg.client_id, challenge, username, code, session),
    )
    .await?;
    let r = resp.result.ok_or("auth:no_result")?;
    Ok(to_tokens(r, None, now_secs))
}

/// The answer field is challenge-specific — `SOFTWARE_TOKEN_MFA_CODE` for TOTP, `SMS_MFA_CODE` for
/// SMS. Sending the wrong key yields a generic `InvalidParameterException`, so it is derived from
/// the challenge name rather than hardcoded.
pub fn mfa_body(
    client_id: &str,
    challenge: &str,
    username: &str,
    code: &str,
    session: &str,
) -> Value {
    let code_field = if challenge == "SMS_MFA" {
        "SMS_MFA_CODE"
    } else {
        "SOFTWARE_TOKEN_MFA_CODE"
    };
    json!({
        "ChallengeName": challenge,
        "ClientId": client_id,
        "ChallengeResponses": { "USERNAME": username, code_field: code },
        "Session": session
    })
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

    /// The whole point of `cognito_error`: an expired temporary password and a genuinely wrong
    /// password are both `NotAuthorizedException`, and only Cognito's `message` tells them apart.
    /// Losing it is what made "check your password" the advice for an account that needed an admin
    /// reset.
    #[test]
    fn cognito_error_keeps_the_type_and_the_message() {
        let body = r#"{"__type":"NotAuthorizedException","message":"Temporary password has expired and must be reset by an administrator."}"#;
        let e = cognito_error(400, body);
        assert!(e.starts_with("auth:cognito:NotAuthorizedException:"));
        assert!(e.contains("Temporary password has expired"));
    }

    #[test]
    fn cognito_error_strips_the_type_namespace() {
        let body = r#"{"__type":"com.amazonaws.cognito.identity#UserNotFoundException","message":"User does not exist."}"#;
        assert!(cognito_error(400, body).starts_with("auth:cognito:UserNotFoundException:"));
    }

    /// An unparseable body must keep its bytes rather than be reported as some invented type.
    #[test]
    fn cognito_error_falls_back_to_the_raw_body() {
        let e = cognito_error(503, "<html>gateway</html>");
        assert_eq!(e, "auth:cognito:503:<html>gateway</html>");
    }

    #[test]
    fn mfa_body_picks_the_code_field_per_challenge() {
        let totp = mfa_body("c1", "SOFTWARE_TOKEN_MFA", "u@e.test", "123456", "sess");
        assert_eq!(
            totp["ChallengeResponses"]["SOFTWARE_TOKEN_MFA_CODE"],
            "123456"
        );
        assert_eq!(totp["ChallengeName"], "SOFTWARE_TOKEN_MFA");

        let sms = mfa_body("c1", "SMS_MFA", "u@e.test", "654321", "sess");
        assert_eq!(sms["ChallengeResponses"]["SMS_MFA_CODE"], "654321");
        // The TOTP field must NOT be present on an SMS answer — Cognito rejects the pair.
        assert!(sms["ChallengeResponses"]["SOFTWARE_TOKEN_MFA_CODE"].is_null());
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
