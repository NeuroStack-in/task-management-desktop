//! Small guards.

/// Validate a dashboard URL before handing it to the OS opener: only a plain **https** web URL, no
/// other scheme and no embedded credentials — so a poisoned config value can't launch an arbitrary
/// program/scheme or smuggle creds (BUILD-PLAN §M6, "dashboard-URL sanitization").
pub fn sanitize_dashboard_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains('@') || host.contains(' ') {
        return None;
    }
    Some(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_https_web_urls_pass() {
        assert!(sanitize_dashboard_url("https://app.workpulse.com/dashboard").is_some());
        assert!(sanitize_dashboard_url("http://app.workpulse.com").is_none()); // not https
        assert!(sanitize_dashboard_url("file:///etc/passwd").is_none()); // scheme
        assert!(sanitize_dashboard_url("javascript:alert(1)").is_none()); // scheme
        assert!(sanitize_dashboard_url("https://user:pass@evil.com").is_none());
        // creds
    }
}
