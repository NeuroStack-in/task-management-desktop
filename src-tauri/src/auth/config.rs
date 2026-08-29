//! Cognito + backend endpoint config. **No secrets are ever baked into the binary** — but all three
//! values here have build-time defaults, because none of them is a secret and an installer that
//! can't reach its own backend is useless.
//!
//! Runtime environment (incl. `.env`) always wins; the baked value is the fallback.
//!
//! ## Why the App Client ID has a default now
//!
//! It used to have none, so `client_id` was empty in every shipped build: `release.yml` injects
//! nothing, the NSIS bundle carries no `.env`, and a clean install therefore has no way to know it.
//! Every employee who downloaded the agent got *"This agent isn't configured for your organization
//! yet"* and could never sign in — the failure only looked configurable because developers happened
//! to have `.env` in their working directory.
//!
//! A Cognito **app-client id is a public identifier**, not a credential: the web app ships the same
//! value inside its browser bundle (`NEXT_PUBLIC_COGNITO_CLIENT_ID`) and it is committed in
//! `.env.example`. Treating it as a secret bought no security and broke the product.

#[derive(Clone, Debug)]
pub struct CognitoConfig {
    pub region: String,
    pub client_id: String,
    pub ingest_url: String,
    /// Hosted-UI host (no scheme), e.g. `wp-workpulse-dev.auth.ap-south-1.amazoncognito.com`. Only
    /// the Google sign-in flow uses it; password auth talks to the regional IdP endpoint directly.
    pub hosted_ui_host: String,
}

/// Fixed loopback port the desktop Google flow catches the Hosted-UI redirect on. Must match a
/// `http://localhost:<port>/callback` entry in the Cognito app client's callback URLs.
pub const OAUTH_LOOPBACK_PORT: u16 = 8788;

/// `option_env!` → a plain `&str`. A variable that is *set but blank* (an unset GitHub `vars.*`
/// expands to exactly that) yields `Some("")`, so emptiness is what callers check — not `None`.
const fn baked(v: Option<&'static str>) -> &'static str {
    match v {
        Some(s) => s,
        None => "",
    }
}

// Baked at build time by `release.yml`, one set per target environment.
const BUILD_CLIENT_ID: &str = baked(option_env!("WP_COGNITO_CLIENT_ID"));
const BUILD_REGION: &str = baked(option_env!("WP_COGNITO_REGION"));
const BUILD_INGEST_URL: &str = baked(option_env!("WP_INGEST_URL"));
const BUILD_HOSTED_UI: &str = baked(option_env!("WP_COGNITO_HOSTED_UI"));

// The shared `dev` stack — the fallback when a build bakes nothing. All three point at the *same*
// environment, so an unconfigured build is coherent rather than half-aimed at two stacks.
const DEV_CLIENT_ID: &str = "1b13u9ic3740m56ilmmrsfng7";
const DEV_REGION: &str = "ap-south-1";
const DEV_INGEST_URL: &str = "https://oqlla6l5oc.execute-api.ap-south-1.amazonaws.com";
const DEV_HOSTED_UI: &str = "wp-workpulse-dev.auth.ap-south-1.amazoncognito.com";

/// Build-time value if the build supplied one, else the dev fallback.
fn baked_or(build: &'static str, dev: &'static str) -> &'static str {
    if build.is_empty() {
        dev
    } else {
        build
    }
}

impl CognitoConfig {
    /// Resolution order per value: **runtime environment** (incl. `.env`) → **build-time** → the dev
    /// stack. Runtime first so a developer can point a release build at another stack without
    /// rebuilding; build-time next so a downloaded installer works on a machine with no environment
    /// at all, which is the case this ordering exists for.
    pub fn from_env() -> Self {
        CognitoConfig {
            region: env_or("WP_COGNITO_REGION", baked_or(BUILD_REGION, DEV_REGION)),
            client_id: env_or(
                "WP_COGNITO_CLIENT_ID",
                baked_or(BUILD_CLIENT_ID, DEV_CLIENT_ID),
            ),
            ingest_url: env_or("WP_INGEST_URL", baked_or(BUILD_INGEST_URL, DEV_INGEST_URL)),
            hosted_ui_host: env_or(
                "WP_COGNITO_HOSTED_UI",
                baked_or(BUILD_HOSTED_UI, DEV_HOSTED_UI),
            ),
        }
    }

    /// The regional Cognito Identity Provider endpoint.
    pub fn idp_endpoint(&self) -> String {
        format!("https://cognito-idp.{}.amazonaws.com/", self.region)
    }

    /// A Hosted-UI URL (`/oauth2/authorize`, `/oauth2/token`, …) for the Google sign-in flow.
    pub fn hosted_ui_url(&self, path: &str) -> String {
        format!("https://{}{}", self.hosted_ui_host, path)
    }

    /// Auth cannot proceed without an App Client ID. Kept as a guard rather than removed: it now
    /// only trips if a build *deliberately* strips the default (e.g. a future white-label build that
    /// must be configured per customer), and failing with `auth:not_configured` is far better than
    /// silently attempting Cognito calls with no client.
    pub fn is_configured(&self) -> bool {
        !self.client_id.is_empty()
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The regression this guards: a shipped build with no environment must still know its app
    /// client, or every clean install shows "not configured for your organization" and no employee
    /// can sign in.
    #[test]
    fn a_build_with_no_env_still_resolves_every_value() {
        assert!(!baked_or(BUILD_CLIENT_ID, DEV_CLIENT_ID).is_empty());
        assert!(!baked_or(BUILD_REGION, DEV_REGION).is_empty());
        assert!(!baked_or(BUILD_INGEST_URL, DEV_INGEST_URL).is_empty());
    }

    /// A set-but-blank build value must not defeat the fallback — that is exactly what an unset
    /// GitHub `vars.*` expands to in the workflow.
    #[test]
    fn a_blank_build_value_falls_back() {
        assert_eq!(baked_or("", DEV_CLIENT_ID), DEV_CLIENT_ID);
        assert_eq!(baked_or("abc", DEV_CLIENT_ID), "abc");
    }
}
