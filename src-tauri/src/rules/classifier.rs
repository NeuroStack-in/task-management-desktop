//! First-match-wins app/URL classification, run on-device. Real logic, no OS dependency — so it is
//! the one capture slice with unit tests.

use serde::{Deserialize, Serialize};
use wp_agent_contract::Category;

/// One app/URL → category rule. In the 3-process design this was `agent_shared::ipc::CategoryRule`;
/// one process needs no IPC crate, so it lives here. M4 derives these from the contract's
/// `AppUrlRules` (apps + urls, URL beats app) when applying synced config.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CategoryRule {
    /// Process name or domain/glob.
    pub matcher: String,
    pub category: Category,
}

/// Classify an app name (or URL/domain) against ordered rules; first match wins, else `Neutral`.
pub fn classify(target: &str, rules: &[CategoryRule]) -> Category {
    let t = target.to_lowercase();
    for r in rules {
        if t.contains(&r.matcher.to_lowercase()) {
            return r.category;
        }
    }
    Category::Neutral
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(m: &str, c: Category) -> CategoryRule {
        CategoryRule {
            matcher: m.into(),
            category: c,
        }
    }

    #[test]
    fn first_match_wins_else_neutral() {
        let rules = vec![
            rule("code", Category::Productive),
            rule("youtube.com", Category::Distracting),
        ];
        assert_eq!(classify("VS Code", &rules), Category::Productive);
        assert_eq!(classify("youtube.com/watch", &rules), Category::Distracting);
        assert_eq!(classify("some-unknown-app", &rules), Category::Neutral);
    }
}
