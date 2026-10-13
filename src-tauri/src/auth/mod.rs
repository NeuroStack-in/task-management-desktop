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
pub mod oauth;
pub mod token;
pub mod token_store;

use std::sync::RwLock;
use std::time::Duration;

use serde::Serialize;

use config::CognitoConfig;
use token::Tokens;

/// Keyring key for the long-lived refresh secret.
const REFRESH_KEY: &str = "refresh_token";

/// How long a Google sign-in waits for the browser deep link before giving up.
const OAUTH_TIMEOUT: Duration = Duration::from_secs(300);

pub struct AuthManager {
    cfg: CognitoConfig,
    http: reqwest::Client,
    tokens: RwLock<Option<Tokens>>,
    /// Serializes refresh so N racing callers do one network refresh, not N.
    refresh_lock: tokio::sync::Mutex<()>,
    /// The one-shot a pending `login_google` is waiting on; the deep-link handler fulfils it with the
    /// `workpulse://callback?…` URL. `None` when no sign-in is in flight. Std mutex, never held across
    /// an await, so the sync deep-link handler can drop a URL in without blocking.
    oauth_wait: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>,
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
            oauth_wait: std::sync::Mutex::new(None),
        }
    }

    pub fn config(&self) -> &CognitoConfig {
        &self.cfg
    }

    pub fn is_authenticated(&self) -> bool {
        self.tokens.read().unwrap().is_some()
    }

    /// The signed-in user's Cognito `sub`, from the ID token. `None` when signed out or undecodable.
    /// What the outbox's owner is recorded as (`AppState::adopt_signed_in`).
    pub fn subject(&self) -> Option<String> {
        let guard = self.tokens.read().unwrap();
        let t = guard.as_ref()?;
        token::decode_id_claims(&t.id_token)
            .map(|c| c.sub)
            .filter(|s| !s.trim().is_empty())
    }

    /// Whether the signed-in user's role tracks their own time (`TimeTrackSelf`, bit 110 of the
    /// `perm` claim). `None` when signed out or the claim can't be read — the server stays the
    /// boundary, so an unreadable claim is never treated as a refusal.
    pub fn can_track_time(&self) -> Option<bool> {
        let guard = self.tokens.read().unwrap();
        let t = guard.as_ref()?;
        token::decode_id_claims(&t.id_token)?.has_permission_bit(token::TIME_TRACK_SELF_BIT)
    }

    /// The server answered 401 to the cached ID token: treat it as expired so the next
    /// [`Self::id_token`] refreshes instead of re-sending it.
    ///
    /// Freshness used to be judged only by the locally computed `expires_at`, so after a backward
    /// clock step (or a token the server revoked early) every call 401'd until the local clock
    /// caught up (QA audit 2026-10-08, WP-442). The refresh token is untouched — only Cognito
    /// rejecting *that* signs the user out.
    pub fn invalidate_id_token(&self) {
        if let Some(t) = self.tokens.write().unwrap().as_mut() {
            t.expires_at = 0;
        }
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
                // Checked and stored under one write lock, so a login cannot slip in between.
                let mut guard = self.tokens.write().unwrap();
                if guard.is_none() {
                    store(&mut guard, t);
                }
                true
            }
            Err(e) => {
                // Only forget the refresh token when Cognito actually rejected it. This comment used
                // to say exactly that while the code cleared unconditionally — so a laptop that woke
                // or booted before Wi-Fi associated failed the startup refresh with a *network*
                // error, threw away a perfectly good refresh token, and demanded a password. The
                // token was never the problem, and re-typing it "fixed" nothing.
                if refresh_failure_is_terminal(&e) {
                    tracing::warn!("stored session was rejected ({e}) — signing out");
                    let _ = token_store::clear(REFRESH_KEY);
                } else {
                    // Keep the token and stay signed out for this attempt; the next restore, or the
                    // sender's own refresh once the network is back, picks the session up again.
                    tracing::warn!("could not reach Cognito to restore the session ({e}) — keeping the stored token and retrying later");
                }
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

    /// Sign in through the **Hosted UI (Google)** — a native **deep-link + PKCE** flow ([`oauth`]).
    ///
    /// `open` launches the system browser (injected so the auth core stays free of the Tauri handle);
    /// the browser round-trips through Google and Cognito redirects to `workpulse://callback?…`,
    /// which the OS hands to the app — the deep-link handler calls [`deliver_oauth_redirect`], which
    /// fulfils the one-shot this awaits. On success the tokens are stored exactly as a password login
    /// stores them, so refresh, claims, and the fleet heartbeat are unchanged.
    ///
    /// [`deliver_oauth_redirect`]: Self::deliver_oauth_redirect
    pub async fn login_google<F>(&self, open: F) -> Result<AuthStatus, String>
    where
        F: FnOnce(&str) -> Result<(), String>,
    {
        let (verifier, challenge) = oauth::pkce();
        let state = oauth::random_b64url(24);
        let url = oauth::authorize_url(&self.cfg, &challenge, &state)?;

        // Arm the one-shot BEFORE opening the browser, so a very fast redirect can't beat us to it.
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        *self.oauth_wait.lock().unwrap() = Some(tx);
        if let Err(e) = open(&url) {
            *self.oauth_wait.lock().unwrap() = None; // browser didn't open — disarm
            return Err(e);
        }

        let redirect = match tokio::time::timeout(OAUTH_TIMEOUT, rx).await {
            Ok(Ok(u)) => u,
            // Sender dropped (a new sign-in armed over this one) — treat as cancelled.
            Ok(Err(_)) => return Err("auth:oauth: sign-in was cancelled".into()),
            Err(_) => {
                *self.oauth_wait.lock().unwrap() = None;
                return Err("auth:oauth: timed out waiting for the browser sign-in".into());
            }
        };

        let cb = oauth::parse_callback(&redirect);
        if let Some(code) = cb.error {
            // Keep the OAuth code as a stable tag the UI can branch on, and append Cognito's own
            // sentence when it sent one — that description is the actual diagnosis (a failed Lambda
            // trigger, a config problem), where the bare `invalid_request` alone tells a person
            // nothing.
            return Err(match cb.error_description {
                Some(d) if !d.trim().is_empty() => format!("auth:oauth:{code}: {d}"),
                _ => format!("auth:oauth:{code}"),
            });
        }
        if cb.state.as_deref() != Some(state.as_str()) {
            return Err("auth:oauth:state_mismatch: sign-in couldn't be verified".into());
        }
        let code = cb
            .code
            .ok_or("auth:oauth:no_code: the sign-in returned no authorization code")?;
        let t = oauth::exchange(&self.http, &self.cfg, &code, &verifier).await?;

        // Open self-signup (the `presignup` trigger) lets a brand-new Google account in with **no
        // org** — correct on the web, where onboarding then creates one, but the agent has nothing to
        // track for an org-less account. Reject it with a clear code instead of signing in to an empty,
        // unusable session. A decode failure is not treated as org-less (we can't tell — let it
        // through and let downstream auth speak).
        if token::decode_id_claims(&t.id_token)
            .map(|c| c.tenant_id.trim().is_empty())
            .unwrap_or(false)
        {
            return Err("auth:oauth:no_org".into());
        }

        self.set(t);
        Ok(self.status())
    }

    /// Hand a `workpulse://callback?…` deep-link URL to a pending [`login_google`]. Called from the
    /// OS deep-link / single-instance handlers. A no-op if no sign-in is waiting (a stray deep link)
    /// or it already resolved — the one-shot is taken exactly once.
    pub fn deliver_oauth_redirect(&self, url: String) {
        if let Ok(mut guard) = self.oauth_wait.lock() {
            if let Some(tx) = guard.take() {
                let _ = tx.send(url);
            }
        }
    }

    /// Abandon a Google sign-in that is still waiting on the browser.
    ///
    /// Dropping the one-shot sender is the whole mechanism: the `rx` inside [`login_google`] wakes
    /// immediately with a `RecvError`, which that function already reports as
    /// `auth:oauth: sign-in was cancelled` — the same path a superseding sign-in takes. Nothing new
    /// has to be plumbed through; the wait simply ends.
    ///
    /// **Why this exists.** The only other way out of that wait was `OAUTH_TIMEOUT`, five minutes.
    /// Someone who opens Google sign-in and then closes the tab — thinks better of it, picks the
    /// wrong account, or just changes their mind — had no way to say so, and the card sat disabled
    /// on "Opening browser…" with both buttons dead. Five minutes of a frozen sign-in screen reads
    /// as a hung app, and the honest response to it is to kill the agent.
    ///
    /// Returns whether a sign-in was actually waiting, so a stray cancel is distinguishable from a
    /// real one rather than silently reported as success.
    pub fn cancel_oauth(&self) -> bool {
        match self.oauth_wait.lock() {
            Ok(mut guard) => guard.take().is_some(),
            // Poisoned only if a previous holder panicked while armed; there is then no live wait to
            // cancel, and reporting `false` is truthful.
            Err(_) => false,
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
        // The keyring is cleared under the same write lock that drops the tokens, so a refresh
        // finishing concurrently can't write the departing user's refresh token back in between.
        let mut guard = self.tokens.write().unwrap();
        *guard = None;
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
        // Both outcomes below apply only if the stored session is still the one we refreshed. A
        // sign-out — or a sign-in as someone else — can land during the round-trip, and writing the
        // result back regardless restored the previous user's tokens to memory and the keyring about
        // a second after they signed out (QA audit 2026-10-08, WP-441). `restore` already re-checked
        // for this; this path did not.
        match cognito::refresh(&self.http, &self.cfg, &refresh_token, now_secs()).await {
            Ok(t) => {
                let id = t.id_token.clone();
                self.replace_if_current(&refresh_token, Some(t))
                    .then_some(id)
            }
            Err(e) if refresh_failure_is_terminal(&e) => {
                // Cognito rejected the token: the session really is over.
                tracing::warn!("session refresh was rejected ({e}) — signing out");
                self.replace_if_current(&refresh_token, None);
                None
            }
            Err(e) => {
                // We could not *reach* Cognito. This ran on every send cycle and called `logout()`,
                // which wipes the keyring — so one blip while the ID token happened to be within its
                // 60 s expiry skew (a laptop resuming, a VPN reconnecting, a captive portal) ended
                // the session and made the user sign in again. Nothing was wrong with the
                // credentials, and the outbox had work it could have sent minutes later.
                //
                // Returning `None` skips this cycle without touching stored state: the tokens stay in
                // memory and in the keyring, and the next cycle refreshes normally.
                //
                // The caller treats `None` as "signed out" for the `auth:expired` edge, which is
                // still the honest signal — the agent cannot send right now. What it must not do is
                // make that momentary truth permanent.
                tracing::warn!("could not reach Cognito to refresh ({e}) — keeping the session and retrying next cycle");
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
        // `debug`, not `info`: the panel calls this every second on its core poll, so at `info` it
        // wrote ~14k identical lines a day and buried every other diagnostic. Presence-only (never
        // the values) — still enough to tell whether the ID token carries `name`/`email`.
        tracing::debug!(
            has_name = !claims.name.trim().is_empty(),
            has_email = !claims.email.trim().is_empty(),
            "identity: resolving display name from ID-token claims"
        );
        Some((claims.display_name(), claims.email.clone()))
    }

    fn set(&self, t: Tokens) {
        store(&mut self.tokens.write().unwrap(), t);
    }

    /// Store `new` — or sign out, for `None` — **only if** the stored session is still the one whose
    /// refresh token is `expected_refresh`. Returns whether it was (WP-441).
    fn replace_if_current(&self, expected_refresh: &str, new: Option<Tokens>) -> bool {
        let mut guard = self.tokens.write().unwrap();
        if guard.as_ref().map(|t| t.refresh_token.as_str()) != Some(expected_refresh) {
            return false;
        }
        match new {
            Some(t) => store(&mut guard, t),
            None => {
                *guard = None;
                let _ = token_store::clear(REFRESH_KEY);
            }
        }
        true
    }
}

/// Persist the refresh token and install `t` as the session, **under the caller's write lock** — so a
/// login, a refresh and a sign-out can never interleave their keyring and memory writes and leave the
/// two naming different users.
fn store(slot: &mut Option<Tokens>, t: Tokens) {
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
    *slot = Some(t);
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

/// Did Cognito *reject* the refresh token, or did we merely fail to ask?
///
/// Only a rejection justifies deleting the stored token, because only a rejection means it will
/// never work again. Everything else — no DNS, captive portal, TLS failure, a 5xx, a malformed body
/// — is a statement about the network at that instant, and the same token will very likely refresh
/// fine a minute later.
///
/// The error strings come from `cognito.rs`, which prefixes them by cause: `auth:network:…`,
/// `auth:parse:…`, and `auth:cognito:<Type>[:message]` for anything Cognito itself answered. Note
/// that the `auth:cognito:` prefix alone is *not* enough — `cognito_error` also falls back to
/// `auth:cognito:<status>:<body>` for a raw gateway 5xx, which is a transport failure wearing
/// Cognito's prefix. So this matches the two exception types that are genuinely terminal.
fn refresh_failure_is_terminal(err: &str) -> bool {
    const TERMINAL: [&str; 2] = [
        // The refresh token was revoked, expired, or belongs to a disabled user.
        "auth:cognito:NotAuthorizedException",
        // The user was deleted from the pool — no future refresh can succeed either.
        "auth:cognito:UserNotFoundException",
    ];
    TERMINAL.iter().any(|t| err.starts_with(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this guards: a machine that boots or wakes before Wi-Fi associates fails the startup
    /// refresh on the network, and used to have its refresh token deleted for it — turning a
    /// momentary outage into a password prompt.
    #[test]
    fn a_network_failure_never_discards_the_stored_session() {
        assert!(!refresh_failure_is_terminal(
            "auth:network:error sending request for url (https://cognito-idp…)"
        ));
        assert!(!refresh_failure_is_terminal(
            "auth:parse:expected value at line 1"
        ));
    }

    /// A gateway 5xx arrives under the `auth:cognito:` prefix via `cognito_error`'s status fallback.
    /// Prefix-matching alone would read it as a rejection and sign the user out over a bad minute at
    /// the load balancer.
    #[test]
    fn a_gateway_error_wearing_the_cognito_prefix_is_not_terminal() {
        assert!(!refresh_failure_is_terminal(
            "auth:cognito:503:<html>gateway</html>"
        ));
        assert!(!refresh_failure_is_terminal("auth:cognito:500:internal"));
    }

    /// A revoked or expired refresh token, and a deleted user, are the two cases where the stored
    /// token really is dead and keeping it would just fail forever.
    #[test]
    fn a_real_rejection_is_terminal() {
        assert!(refresh_failure_is_terminal(
            "auth:cognito:NotAuthorizedException:Refresh Token has expired."
        ));
        assert!(refresh_failure_is_terminal(
            "auth:cognito:UserNotFoundException:User does not exist."
        ));
    }

    /// Throttling is explicitly survivable — Cognito asking us to slow down must not cost the session.
    #[test]
    fn throttling_is_not_terminal() {
        assert!(!refresh_failure_is_terminal(
            "auth:cognito:TooManyRequestsException:Rate exceeded"
        ));
    }

    fn tokens(refresh: &str) -> Tokens {
        Tokens {
            id_token: "id".into(),
            access_token: "ac".into(),
            refresh_token: refresh.into(),
            expires_at: i64::MAX,
        }
    }

    /// WP-441: a refresh that finishes after the session changed must not touch the new state — not
    /// write the old tokens back, and not sign the new session out on the old one's rejection.
    #[test]
    fn a_stale_refresh_leaves_a_changed_session_alone() {
        let m = AuthManager::new();
        assert!(!m.replace_if_current("old", Some(tokens("old"))));
        assert!(
            !m.is_authenticated(),
            "signed out meanwhile: stays signed out"
        );

        *m.tokens.write().unwrap() = Some(tokens("new"));
        assert!(!m.replace_if_current("old", None));
        assert_eq!(
            m.tokens.read().unwrap().as_ref().unwrap().refresh_token,
            "new",
            "another user signed in meanwhile: their session survives"
        );
    }

    /// WP-442: a 401 makes the next `id_token()` refresh rather than resend the same token.
    #[test]
    fn a_server_rejection_expires_the_cached_token() {
        let m = AuthManager::new();
        *m.tokens.write().unwrap() = Some(tokens("rt"));
        m.invalidate_id_token();
        let guard = m.tokens.read().unwrap();
        let t = guard.as_ref().unwrap();
        assert!(t.is_expired(now_secs(), 60));
        assert_eq!(t.refresh_token, "rt", "the refresh token is kept");
    }

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
