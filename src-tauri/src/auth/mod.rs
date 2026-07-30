//! Authentication — the agent logs in **as the user** (Cognito `USER_PASSWORD_AUTH`); each batch is
//! attributed to `auth.user_id` (backend CLAUDE.md; per-device enrollment is deferred,
//! `docs/ENROLLMENT.md`). The refresh token lives in the OS keyring; the ID token (with the claims)
//! is refreshed **single-flight** behind one async mutex. A failed refresh signs out and the UI must
//! return to login (`events::AUTH_EXPIRED`).
//!
//! **Not live-verified here** — needs the real pool + `WP_COGNITO_CLIENT_ID` + a real password
//! (`owner@acme.test`). The pure parts (bodies, claim decode, keyring chunking) are unit-tested.

pub mod cognito;
pub mod config;
pub mod token;
pub mod token_store;

use std::sync::RwLock;
use std::time::Duration;

use serde::Serialize;

use config::CognitoConfig;
use token::Tokens;

/// Keyring key for the long-lived refresh secret.
const REFRESH_KEY: &str = "refresh_token";

pub struct AuthManager {
    cfg: CognitoConfig,
    http: reqwest::Client,
    tokens: RwLock<Option<Tokens>>,
    /// Serializes refresh so N racing callers do one network refresh, not N.
    refresh_lock: tokio::sync::Mutex<()>,
}

impl AuthManager {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        AuthManager {
            cfg: CognitoConfig::from_env(),
            http,
            tokens: RwLock::new(None),
            refresh_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub fn config(&self) -> &CognitoConfig {
        &self.cfg
    }

    pub fn is_authenticated(&self) -> bool {
        self.tokens.read().unwrap().is_some()
    }

    /// Restore a session at startup from the keyring's refresh token. Returns whether it worked.
    ///
    /// **Never clobbers a live session.** `restore` is spawned async at launch and its Cognito
    /// refresh is a network round-trip, so the user can finish signing in (as a *different* person)
    /// before it returns. If that happened, keeping the freshly-established session is the only
    /// correct outcome — otherwise the just-logged-in user's API calls would silently go out under
    /// the previous user's token (their projects, their tasks, their data).
    pub async fn restore(&self) -> bool {
        if self.is_authenticated() {
            return true;
        }
        let Some(rt) = token_store::load(REFRESH_KEY).and_then(|b| String::from_utf8(b).ok())
        else {
            return false;
        };
        match cognito::refresh(&self.http, &self.cfg, &rt, now_secs()).await {
            Ok(t) => {
                // Re-check after the network round-trip: a login may have landed while we refreshed.
                if self.is_authenticated() {
                    return true;
                }
                self.set(t);
                true
            }
            Err(e) => {
                // A refresh can fail because the token genuinely expired or was revoked — clearing
                // is right — but it can also be a transient network failure at startup, so say which
                // rather than silently demanding a password.
                tracing::warn!("stored session could not be refreshed ({e}) — signing out");
                let _ = token_store::clear(REFRESH_KEY);
                false
            }
        }
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<AuthStatus, String> {
        match cognito::login(&self.http, &self.cfg, username, password, now_secs()).await? {
            cognito::LoginOutcome::Tokens(t) => {
                self.set(t);
                Ok(self.status())
            }
            cognito::LoginOutcome::NewPasswordRequired { session } => {
                Ok(AuthStatus::new_password(session))
            }
            cognito::LoginOutcome::MfaRequired { challenge, session } => {
                Ok(AuthStatus::mfa(challenge, session))
            }
        }
    }

    /// Answer an outstanding MFA challenge. On success the tokens are stored exactly as a
    /// password-only login would store them, so everything downstream is unchanged.
    pub async fn complete_mfa(
        &self,
        challenge: &str,
        username: &str,
        code: &str,
        session: &str,
    ) -> Result<AuthStatus, String> {
        let t = cognito::respond_mfa(
            &self.http,
            &self.cfg,
            challenge,
            username,
            code,
            session,
            now_secs(),
        )
        .await?;
        self.set(t);
        Ok(self.status())
    }

    pub async fn complete_new_password(
        &self,
        username: &str,
        new_password: &str,
        session: &str,
    ) -> Result<AuthStatus, String> {
        let t = cognito::complete_new_password(
            &self.http,
            &self.cfg,
            username,
            new_password,
            session,
            now_secs(),
        )
        .await?;
        self.set(t);
        Ok(self.status())
    }

    pub fn logout(&self) {
        *self.tokens.write().unwrap() = None;
        let _ = token_store::clear(REFRESH_KEY);
    }

    /// A valid ID token to attach to a request, refreshing single-flight if it is expired/near-expiry.
    /// `None` when signed out (or a refresh failed → signed out).
    pub async fn id_token(&self) -> Option<String> {
        // Fast path: a still-valid token.
        {
            let guard = self.tokens.read().unwrap();
            let t = guard.as_ref()?;
            if !t.is_expired(now_secs(), 60) {
                return Some(t.id_token.clone());
            }
        }
        // Slow path: one refresh at a time.
        let _lock = self.refresh_lock.lock().await;
        let refresh_token = {
            let guard = self.tokens.read().unwrap();
            let t = guard.as_ref()?;
            if !t.is_expired(now_secs(), 60) {
                return Some(t.id_token.clone()); // someone else refreshed while we waited
            }
            t.refresh_token.clone()
        };
        match cognito::refresh(&self.http, &self.cfg, &refresh_token, now_secs()).await {
            Ok(t) => {
                let id = t.id_token.clone();
                self.set(t);
                Some(id)
            }
            Err(_) => {
                self.logout();
                None
            }
        }
    }

    /// Current auth status for the UI.
    pub fn status(&self) -> AuthStatus {
        let guard = self.tokens.read().unwrap();
        match guard.as_ref() {
            None => AuthStatus::signed_out(),
            Some(t) => {
                let claims = token::decode_id_claims(&t.id_token);
                AuthStatus {
                    signed_in: true,
                    tenant_id: claims.as_ref().map(|c| c.tenant_id.clone()),
                    username: claims.as_ref().map(|c| c.username.clone()),
                    ..AuthStatus::signed_out()
                }
            }
        }
    }

    /// The signed-in user's display name + email for the panel, from the ID-token claims. `None` when
    /// signed out. Uses the `name`/`email` claims — **never** the Cognito `username`/`sub` UUID, which
    /// is what made the header show a hex string.
    pub fn identity(&self) -> Option<(String, String)> {
        let guard = self.tokens.read().unwrap();
        let t = guard.as_ref()?;
        let claims = token::decode_id_claims(&t.id_token)?;
        // Diagnostic (presence only, not values): if the header still shows a UUID, this tells us the
        // ID token carries no `name`/`email` claim — a Cognito app-client attribute config, not an
        // agent bug. Falls back to the username only when both are absent.
        tracing::info!(
            has_name = !claims.name.trim().is_empty(),
            has_email = !claims.email.trim().is_empty(),
            "identity: resolving display name from ID-token claims"
        );
        Some((claims.display_name(), claims.email.clone()))
    }

    fn set(&self, t: Tokens) {
        // **Logged, not discarded.** A failed keyring write is not cosmetic: it is the difference
        // between "stay signed in" and "ask for a password on every launch", and it is otherwise
        // completely silent — the session works fine until the next restart, so the symptom shows up
        // far from the cause. This was `let _ = …` while every write was failing on Windows for an
        // oversized chunk (see `token_store::CHUNK`), and nothing anywhere said so.
        if let Err(e) = token_store::store(REFRESH_KEY, t.refresh_token.as_bytes()) {
            tracing::error!(
                "could not persist the refresh token ({e}) — sign-in will not survive a restart"
            );
        }
        *self.tokens.write().unwrap() = Some(t);
    }
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

fn now_secs() -> i64 {
    crate::clock::now_epoch_ms() / 1000
}

/// What the webview shows: signed-in (+ who), signed-out, or "set a new password" mid-challenge.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    pub signed_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Present when Cognito requires a new password (first admin-created login) — the UI collects one
    /// and calls `auth_complete_new_password` with this session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_password_session: Option<String>,
    /// Present when Cognito requires a second factor — the UI collects a 6-digit code and calls
    /// `auth_complete_mfa` with this session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mfa_session: Option<String>,
    /// Which challenge is outstanding (`SOFTWARE_TOKEN_MFA` | `SMS_MFA`). Echoed back on the answer,
    /// and it decides the prompt wording ("authenticator app" vs "text message").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mfa_challenge: Option<String>,
}

impl AuthStatus {
    fn new_password(session: String) -> Self {
        AuthStatus {
            new_password_session: Some(session),
            ..AuthStatus::signed_out()
        }
    }

    fn mfa(challenge: String, session: String) -> Self {
        AuthStatus {
            mfa_session: Some(session),
            mfa_challenge: Some(challenge),
            ..AuthStatus::signed_out()
        }
    }

    pub fn signed_out() -> Self {
        AuthStatus {
            signed_in: false,
            tenant_id: None,
            username: None,
            new_password_session: None,
            mfa_session: None,
            mfa_challenge: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UI-read DTO: the serialized names must be camelCase (the boundary the TS side reads).
    #[test]
    fn auth_status_serializes_camelcase() {
        let s = AuthStatus {
            signed_in: true,
            tenant_id: Some("t1".into()),
            username: Some("u".into()),
            ..AuthStatus::signed_out()
        };
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j.get("signedIn"), Some(&serde_json::json!(true)));
        assert!(j.get("tenantId").is_some());
        assert!(j.get("signed_in").is_none());
        assert!(j.get("newPasswordSession").is_none()); // None → skipped
    }
}
