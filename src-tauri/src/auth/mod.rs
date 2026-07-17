//! Authentication — **M1**. The agent logs in **as the user** (Cognito `USER_PASSWORD_AUTH`); each
//! batch is attributed to `auth.user_id` (backend CLAUDE.md; per-device enrollment is deferred,
//! `docs/ENROLLMENT.md`).
//!
//! M1 (BUILD-PLAN): three hand-rolled JSON POSTs over `reqwest` — `InitiateAuth`,
//! `RespondToAuthChallenge` (NEW_PASSWORD_REQUIRED), `REFRESH_TOKEN_AUTH` — **no SRP, no `aws-sdk`**.
//! Tokens in the OS keyring, **base64 + chunked** (`key.0..N` + a count) to clear the Windows
//! Credential Manager blob limit. Single-flight refresh behind one `tokio::sync::Mutex`. A 401
//! emits `events::AUTH_EXPIRED`. The claims ride the **ID token** (Cognito custom claims are strings).

// TODO(M1): cognito.rs (the three flows) + keyring.rs (chunked token store).
