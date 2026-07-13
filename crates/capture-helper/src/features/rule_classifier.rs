//! Applies the synced app/URL category rules to a focus span **on-device** (edge-first: the
//! cloud receives pre-classified spans). Rules ride the CONFIG rail from the server. Real logic,
//! no OS dependency — so it's the one capture slice with unit tests.

use agent_shared::contract::Category;
use agent_shared::ipc::CategoryRule;

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
