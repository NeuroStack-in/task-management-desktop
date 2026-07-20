//! `GET /v1/me/timesheet/today` — the caller's folded time entries for a day, so the panel's
//! "Today's sessions" shows real logged time (BUILD-PLAN §6). Same user Cognito JWT as the batch
//! rail. The server folds this agent's own `TimerStarted/Stopped` events into `TimeEntry` rows
//! (ingest → time-attendance); this reads them back, aggregated per (project, description).
//!
//! "Today" is the **client's** local date (the Lambda runs in UTC), so the caller passes the date.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// One folded timesheet entry (subset of the backend `EntryRow`).
#[derive(Debug, Deserialize)]
struct EntryDto {
    #[serde(default)]
    project_id: String,
    #[serde(default)]
    description: String,
    /// Absent while the session is still running — those are excluded from the completed totals; the
    /// panel folds the live segment from its own timer instead (no double count).
    #[serde(default)]
    duration_secs: Option<i64>,
}

/// A day's sessions, aggregated per (project, description) — the panel's `Session` shape.
#[derive(Debug, Clone, Serialize)]
pub struct SessionDto {
    pub project_id: String,
    pub description: String,
    pub secs: i64,
}

pub async fn fetch_today(
    client: &reqwest::Client,
    ingest_url: &str,
    id_token: &str,
    date: &str,
) -> Result<Vec<SessionDto>, String> {
    let url = format!(
        "{}/v1/me/timesheet/today?date={}",
        ingest_url.trim_end_matches('/'),
        date
    );
    let resp = client
        .get(url)
        .bearer_auth(id_token)
        .send()
        .await
        .map_err(|e| format!("timesheet:network:{e}"))?;
    let status = resp.status();
    if status.as_u16() == 401 {
        return Err("auth:expired".into());
    }
    let text = resp
        .text()
        .await
        .map_err(|e| format!("timesheet:read:{e}"))?;
    if !status.is_success() {
        return Err(format!("timesheet:status:{}:{text}", status.as_u16()));
    }
    Ok(aggregate(parse_entries(&text)?))
}

/// Unwrap `{ "data": { "entries": [...] } }` (the backend `Envelope`), else bare `{ "entries" }`.
fn parse_entries(text: &str) -> Result<Vec<EntryDto>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("timesheet:parse:{e}"))?;
    let inner = value.get("data").cloned().unwrap_or(value);
    let list = inner
        .get("entries")
        .cloned()
        .ok_or("timesheet:parse:missing 'entries' field")?;
    serde_json::from_value(list).map_err(|e| format!("timesheet:parse:{e}"))
}

/// Sum completed entries per (project, description); running ones (no duration) are skipped.
fn aggregate(entries: Vec<EntryDto>) -> Vec<SessionDto> {
    let mut by: HashMap<(String, String), i64> = HashMap::new();
    for e in entries {
        if let Some(secs) = e.duration_secs {
            *by.entry((e.project_id, e.description)).or_default() += secs;
        }
    }
    let mut out: Vec<SessionDto> = by
        .into_iter()
        .map(|((project_id, description), secs)| SessionDto {
            project_id,
            description,
            secs,
        })
        .collect();
    // Longest first — the biggest chunk of the day reads at the top.
    out.sort_by_key(|s| std::cmp::Reverse(s.secs));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_completed_and_skips_running() {
        let json = r#"{"data":{"entries":[
            {"project_id":"p1","description":"Redesign","duration_secs":1800},
            {"project_id":"p1","description":"Redesign","duration_secs":1200},
            {"project_id":"p2","description":"Retry","duration_secs":600},
            {"project_id":"p1","description":"Redesign"}
        ]}}"#;
        let sessions = aggregate(parse_entries(json).unwrap());
        // Two distinct (project, description) buckets; the durationless (running) one is skipped.
        assert_eq!(sessions.len(), 2);
        let redesign = sessions
            .iter()
            .find(|s| s.description == "Redesign")
            .unwrap();
        assert_eq!(redesign.secs, 3000); // 1800 + 1200, running one excluded
        assert_eq!(sessions[0].description, "Redesign"); // longest first
    }

    #[test]
    fn missing_entries_field_is_an_error() {
        assert!(parse_entries(r#"{"data":{}}"#).is_err());
    }
}
