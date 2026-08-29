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
    /// The person's display name, when the pool populates it.
    #[serde(default)]
    pub name: String,
    /// The user's email — the reliable human identifier; its local-part is the display fallback.
    #[serde(default)]
    pub email: String,
}

impl IdClaims {
    /// A human display name for the panel: the `name` claim if present, else the email's local-part
    /// title-cased, else the email, else the raw username. **Never** the Cognito `username`/`sub`
    /// UUID when a real name or email exists — that's the hex string bug this avoids.
    pub fn display_name(&self) -> String {
        let name = self.name.trim();
        if !name.is_empty() {
            return name.to_string();
        }
        let email = self.email.trim();
        if !email.is_empty() {
            return titlecase_local_part(email);
        }
        self.username.clone()
    }
}

/// `alex.morgan@acme.test` → `Alex Morgan`. Splits the local-part on `. _ -` and title-cases; falls
/// back to the whole string if it has no local-part.
fn titlecase_local_part(email: &str) -> String {
    let local = email.split('@').next().unwrap_or(email);
    if local.is_empty() {
        return email.to_string();
    }
    let words: Vec<String> = local
        .split(['.', '_', '-'])
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect();
    if words.is_empty() {
        email.to_string()
    } else {
        words.join(" ")
    }
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

    /// The org-less guard in `login_google` keys off exactly this: a Google shell account (open
    /// self-signup, no org) issues an ID token with **no `tenant_id`**, which must decode to the empty
    /// string so the guard can reject it — not to some non-empty placeholder that reads as a real org.
    #[test]
    fn an_org_less_token_decodes_to_an_empty_tenant() {
        let payload = r#"{"sub":"u1","perm":"0","exp":1720000000,"cognito:username":"new@self.test","email":"new@self.test"}"#;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
        let jwt = format!("header.{b64}.signature");
        let c = decode_id_claims(&jwt).unwrap();
        assert!(c.tenant_id.trim().is_empty());
    }

    fn claims(name: &str, email: &str, username: &str) -> IdClaims {
        IdClaims {
            sub: "u1".into(),
            tenant_id: "t1".into(),
            perm: "3".into(),
            exp: 0,
            username: username.into(),
            name: name.into(),
            email: email.into(),
        }
    }

    #[test]
    fn display_name_prefers_name_then_email_then_username() {
        // Real name wins.
        assert_eq!(
            claims("Alex Morgan", "a@x.io", "uuid-123").display_name(),
            "Alex Morgan"
        );
        // No name → title-cased email local-part, NOT the UUID username.
        assert_eq!(
            claims("", "alex.morgan@acme.test", "a1931dda-0041-703c").display_name(),
            "Alex Morgan"
        );
        assert_eq!(
            claims("", "owner@acme.test", "uuid").display_name(),
            "Owner"
        );
        // Neither name nor email → fall back to username (last resort).
        assert_eq!(claims("", "", "fallback").display_name(), "fallback");
    }

    /// The bug this fixes: a Cognito username that is a UUID must never be rendered as the name when a
    /// real name/email exists.
    #[test]
    fn a_uuid_username_never_becomes_the_display_name_when_email_exists() {
        let c = claims(
            "",
            "jane.doe@corp.com",
            "a1931dda-0041-703c-f41c-300cfaa114dd",
        );
        assert_eq!(c.display_name(), "Jane Doe");
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
