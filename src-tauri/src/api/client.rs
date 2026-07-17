//! The two `reqwest` clients: a 30 s one for the JSON API, a 180 s one for S3 uploads (BUILD-PLAN §4).

use std::time::Duration;

pub fn api_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

pub fn upload_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}
