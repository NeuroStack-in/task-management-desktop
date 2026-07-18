//! Cognito + backend endpoint config, read from the environment — **no secrets or client ids baked
//! into the binary**. The App Client ID has no default (must be provided); region + ingest URL
//! default to the live dev stack (backend/CLAUDE.md).

#[derive(Clone, Debug)]
pub struct CognitoConfig {
    pub region: String,
    pub client_id: String,
    pub ingest_url: String,
}

impl CognitoConfig {
    pub fn from_env() -> Self {
        CognitoConfig {
            region: env_or("WP_COGNITO_REGION", "ap-south-1"),
            client_id: std::env::var("WP_COGNITO_CLIENT_ID").unwrap_or_default(),
            ingest_url: env_or(
                "WP_INGEST_URL",
                "https://oqlla6l5oc.execute-api.ap-south-1.amazonaws.com",
            ),
        }
    }

    /// The regional Cognito Identity Provider endpoint.
    pub fn idp_endpoint(&self) -> String {
        format!("https://cognito-idp.{}.amazonaws.com/", self.region)
    }

    /// Auth cannot proceed without an App Client ID.
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
