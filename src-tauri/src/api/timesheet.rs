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
    /// The task the session was filed against. Always the **parent** task, even when a subtask was
    /// being timed — that is the server contract (`AgentEvent::TimerStarted`).
    #[serde(default)]
    task_id: String,
    /// The subtask, when one was picked. Empty means the timer targeted the task itself.
    #[serde(default)]
    subtask_id: String,
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
    /// Ids, not names. The panel already holds the task list for its picker, so it resolves titles
    /// there rather than making this call fetch them — one join in the UI beats a second round trip
    /// here, and a task that has since been deleted degrades to the description instead of failing.
    pub task_id: String,
    pub subtask_id: String,
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

/// Sum completed entries per (project, task, subtask, description); running ones are skipped.
///
/// **Task and subtask join the key** so the list can name the work rather than only its label. Two
/// sessions with the same description on different subtasks are different work and must not merge —
/// which is exactly what the old (project, description) key did, and why breaking a task down made
/// the day read as one undifferentiated block.
fn aggregate(entries: Vec<EntryDto>) -> Vec<SessionDto> {
    type Key = (String, String, String, String);
    let mut by: HashMap<Key, i64> = HashMap::new();
    for e in entries {
        if let Some(secs) = e.duration_secs {
            *by.entry((e.project_id, e.task_id, e.subtask_id, e.description))
                .or_default() += secs;
        }
    }
    let mut out: Vec<SessionDto> = by
        .into_iter()
        .map(
            |((project_id, task_id, subtask_id, description), secs)| SessionDto {
                project_id,
                task_id,
                subtask_id,
                description,
                secs,
            },
        )
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
            {"project_id":"p1","task_id":"k1","description":"Redesign","duration_secs":1800},
            {"project_id":"p1","task_id":"k1","description":"Redesign","duration_secs":1200},
            {"project_id":"p2","task_id":"k2","description":"Retry","duration_secs":600},
            {"project_id":"p1","task_id":"k1","description":"Redesign"}
        ]}}"#;
        let sessions = aggregate(parse_entries(json).unwrap());
        // Two distinct buckets; the durationless (running) one is skipped.
        assert_eq!(sessions.len(), 2);
        let redesign = sessions
            .iter()
            .find(|s| s.description == "Redesign")
            .unwrap();
        assert_eq!(redesign.secs, 3000); // 1800 + 1200, running one excluded
        assert_eq!(sessions[0].description, "Redesign"); // longest first
    }

    /// The reason task and subtask are in the key. Same project, same description, different
    /// subtasks — that is two different pieces of work, and merging them made a broken-down task
    /// read as one undifferentiated block in the day.
    #[test]
    fn the_same_description_on_two_subtasks_stays_two_rows() {
        let json = r#"{"data":{"entries":[
            {"project_id":"p1","task_id":"k1","subtask_id":"s1","description":"work","duration_secs":600},
            {"project_id":"p1","task_id":"k1","subtask_id":"s2","description":"work","duration_secs":300}
        ]}}"#;
        let sessions = aggregate(parse_entries(json).unwrap());
        assert_eq!(sessions.len(), 2, "different subtasks must not merge");
        assert_eq!(sessions[0].secs, 600); // longest first
        assert_eq!(sessions[0].subtask_id, "s1");
    }

    /// A pre-subtask entry carries neither field; it must still aggregate rather than fail.
    #[test]
    fn entries_without_task_or_subtask_still_aggregate() {
        let json = r#"{"data":{"entries":[
            {"project_id":"p1","description":"old","duration_secs":120},
            {"project_id":"p1","description":"old","duration_secs":60}
        ]}}"#;
        let sessions = aggregate(parse_entries(json).unwrap());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].secs, 180);
        assert_eq!(sessions[0].task_id, "");
    }

    #[test]
    fn missing_entries_field_is_an_error() {
        assert!(parse_entries(r#"{"data":{}}"#).is_err());
    }
}
