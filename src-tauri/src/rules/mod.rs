//! Synced app/URL rules applied **on-device** (edge-first: the cloud receives pre-classified spans).
//! Rules ride the CONFIG rail from the server (contract `AppUrlRules`).
//!
//! M0 keeps the classifier + its test. M4 wires it to its three consumers — set `AppSpan.category`,
//! suppress the screenshot **and** the span for an `exceptions` match, and emit `PolicyViolation`
//! for a `blocked` match — **enforcement only while the timer runs** (off-timer: none, ever;
//! BUILD-PLAN §2).

pub mod classifier;
pub use classifier::{classify, CategoryRule};

use wp_agent_contract::{AppUrlRules, Category};

/// Classify a foreground app against the synced rules. **URL beats app** (LLD §14); `x-win` gives no
/// URL today, so this matches on the app name. Unknown → `Neutral`.
pub fn classify_focus(app: &str, rules: &AppUrlRules) -> Category {
    let a = app.to_lowercase();
    for u in &rules.urls {
        if a.contains(&u.domain.to_lowercase()) {
            return u.category;
        }
    }
    for r in &rules.apps {
        if a.contains(&r.process_name.to_lowercase()) {
            return r.category;
        }
    }
    Category::Neutral
}

/// A matching app rule with `tracked = false` drops the span entirely (LLD §14 / BUILD-PLAN §2).
pub fn is_untracked(app: &str, rules: &AppUrlRules) -> bool {
    let a = app.to_lowercase();
    rules
        .apps
        .iter()
        .any(|r| !r.tracked && a.contains(&r.process_name.to_lowercase()))
}

/// Does the focused window match the org's **restricted** (blocked) lists?
///
/// `haystack` is app + title + URL concatenated — the URL when the platform yields one, the window
/// title as the fallback signal (a domain often appears there), the process name for app rules.
/// Returns the contract's `(kind, identifier)` — `blocked_url` beats `blocked_app`, mirroring
/// "URL beats app" in classification (LLD §14). Checked independently of `is_untracked`: the three
/// rule lists are orthogonal, so an untracked app can still be restricted.
pub fn blocked_match(haystack: &str, rules: &AppUrlRules) -> Option<(&'static str, String)> {
    let h = haystack.to_lowercase();
    for domain in &rules.blocked.urls {
        if !domain.trim().is_empty() && h.contains(&domain.to_lowercase()) {
            return Some(("blocked_url", domain.clone()));
        }
    }
    for process in &rules.blocked.apps {
        if !process.trim().is_empty() && h.contains(&process.to_lowercase()) {
            return Some(("blocked_app", process.clone()));
        }
    }
    None
}

#[cfg(test)]
mod focus_tests {
    use super::*;
    use wp_agent_contract::{AppRule, UrlRule};

    fn rules() -> AppUrlRules {
        AppUrlRules {
            apps: vec![AppRule {
                process_name: "chrome".into(),
                display_name: None,
                category: Category::Neutral,
                tracked: true,
            }],
            urls: vec![UrlRule {
                domain: "youtube.com".into(),
                category: Category::Distracting,
                tracked: true,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn url_rule_beats_app_rule() {
        let r = rules();
        // Matches both the app ("chrome") and the URL ("youtube.com") → URL wins (LLD §14).
        assert_eq!(
            classify_focus("chrome youtube.com", &r),
            Category::Distracting
        );
        assert_eq!(classify_focus("chrome", &r), Category::Neutral); // app-only
        assert_eq!(classify_focus("code", &r), Category::Neutral); // no match
    }

    #[test]
    fn untracked_app_is_dropped() {
        let mut r = rules();
        r.apps[0].tracked = false;
        assert!(is_untracked("chrome", &r));
        assert!(!is_untracked("code", &r));
    }

    #[test]
    fn blocked_match_finds_urls_and_apps_and_prefers_url() {
        let mut r = rules();
        r.blocked.urls = vec!["youtube.com".into()];
        r.blocked.apps = vec!["steam".into()];
        // Domain visible in the title/url haystack → blocked_url with the offending domain.
        assert_eq!(
            blocked_match("chrome Watch - youtube.com", &r),
            Some(("blocked_url", "youtube.com".to_string()))
        );
        assert_eq!(
            blocked_match("steam big picture", &r),
            Some(("blocked_app", "steam".to_string()))
        );
        // A haystack matching both reports the URL (URL beats app, LLD §14).
        r.blocked.apps = vec!["chrome".into()];
        assert_eq!(
            blocked_match("chrome youtube.com", &r).unwrap().0,
            "blocked_url"
        );
        assert_eq!(blocked_match("code main.rs", &r), None);
        // Empty entries never match everything.
        r.blocked.urls = vec!["".into()];
        r.blocked.apps = vec!["  ".into()];
        assert_eq!(blocked_match("anything", &r), None);
    }
}
