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
}
