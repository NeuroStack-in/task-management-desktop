//! Cognito tokens + ID-token claim decoding. The claims are **decoded, not verified** — signature
//! verification is upstream at the API Gateway authorizer; the agent only reads them for display.

use base64::Engine;
use serde::Deserialize;

/// The refresh token is the only long-lived secret (kept in the OS keyring). The **ID token** carries
/// the claims and is what the backend reads (not the access token — backend CLAUDE.md).
#[derive(Clone, Debug)]
pub struct Tokens {
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    /// Epoch **seconds** when the id/access tokens expire.
    pub expires_at: i64,
}

impl Tokens {
    /// Expired (or within `skew_secs` of it) → refresh before use.
    pub fn is_expired(&self, now_secs: i64, skew_secs: i64) -> bool {
        now_secs + skew_secs >= self.expires_at
    }
}

/// Claims the agent reads from the ID token. Cognito custom claims are **strings** (backend CLAUDE.md).
#[derive(Clone, Debug, Deserialize)]
pub struct IdClaims {
    #[serde(default)]
    pub sub: String,
    #[serde(default)]
    pub tenant_id: String,
    #[serde(default)]
    pub perm: String,
    #[serde(default)]
    pub exp: i64,
    #[serde(default, rename = "cognito:username")]
    pub username: String,
}

/// Decode (NOT verify) the ID token's payload segment.
pub fn decode_id_claims(id_token: &str) -> Option<IdClaims> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_string_claims() {
        let payload = r#"{"sub":"u1","tenant_id":"t1","perm":"3","exp":1720000000,"cognito:username":"owner"}"#;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
        let jwt = format!("header.{b64}.signature");
        let c = decode_id_claims(&jwt).unwrap();
        assert_eq!(c.tenant_id, "t1");
        assert_eq!(c.perm, "3");
        assert_eq!(c.sub, "u1");
        assert_eq!(c.username, "owner");
    }

    #[test]
    fn expiry_respects_skew() {
        let t = Tokens {
            id_token: "x".into(),
            access_token: "y".into(),
            refresh_token: "z".into(),
            expires_at: 1000,
        };
        assert!(!t.is_expired(900, 60));
        assert!(t.is_expired(950, 60)); // within skew
    }
}
