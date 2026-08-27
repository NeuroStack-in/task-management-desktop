//! `GET /v1/me/tasks` — the tasks the panel's picker offers. Same user Cognito JWT as the batch
//! rail. The agent needs `id`/`title`/`project_id`/`unassigned`; `status`/`due` are ignored (serde
//! drops unknown fields).
//!
//! **`?include_unassigned=true`.** The panel asks a different question from the web app's "My
//! tasks" card: not *what am I responsible for* but *what could I be working on right now*. A task
//! nobody has taken is exactly that, so the picker offers every project member the unclaimed work in
//! their projects. The flag is opt-in server-side precisely so the web surfaces keep their narrower
//! meaning — see `projects::my_tasks`.
//!
//! Picking one does **not** claim it. Starting a timer records time against the task and leaves it
//! unassigned, so it stays available to everyone; assigning is still a deliberate act on the board.

use serde::{Deserialize, Serialize};

/// One task for the panel picker (subset of the backend `MyTaskRow`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskDto {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub project_id: String,
    /// Nobody has taken this one — it came from the project, not from an assignment.
    ///
    /// Defaults to `false` so a panel talking to a backend that predates the field shows every task
    /// as assigned rather than every task as up-for-grabs. Wrong in the quiet direction.
    #[serde(default)]
    pub unassigned: bool,
}

pub async fn fetch_tasks(
    client: &reqwest::Client,
    ingest_url: &str,
    id_token: &str,
) -> Result<Vec<TaskDto>, String> {
    let url = format!(
        "{}/v1/me/tasks?include_unassigned=true",
        ingest_url.trim_end_matches('/')
    );
    let resp = client
        .get(url)
        .bearer_auth(id_token)
        .send()
        .await
        .map_err(|e| format!("tasks:network:{e}"))?;
    let status = resp.status();
    if status.as_u16() == 401 {
        return Err("auth:expired".into());
    }
    let text = resp.text().await.map_err(|e| format!("tasks:read:{e}"))?;
    if !status.is_success() {
        return Err(format!("tasks:status:{}:{text}", status.as_u16()));
    }
    parse_tasks(&text)
}

/// Unwrap `{ "data": { "tasks": [...] } }` (the backend `Envelope`), else a bare `{ "tasks" }`.
fn parse_tasks(text: &str) -> Result<Vec<TaskDto>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("tasks:parse:{e}"))?;
    let inner = value.get("data").cloned().unwrap_or(value);
    let list = inner
        .get("tasks")
        .cloned()
        .ok_or("tasks:parse:missing 'tasks' field")?;
    serde_json::from_value(list).map_err(|e| format!("tasks:parse:{e}"))
}

/// Optional fields the panel can set when creating a task. Empty strings mean "omit" — the server
/// then applies its own default (unassigned; no due date; `medium` priority).
#[derive(Default)]
pub struct NewTaskFields<'a> {
    pub description: &'a str,
    /// User id to assign (the caller's own `sub` for "assign to me"). Empty = unassigned, which is
    /// the "offer it to everyone on the project" state the picker surfaces.
    pub assignee_id: &'a str,
    /// `YYYY-MM-DD`, or empty for no due date.
    pub due: &'a str,
    /// `low` | `medium` | `high`, or empty to let the server default to `medium`.
    pub priority: &'a str,
}

/// `POST /v1/projects/{project_id}/tasks` — create a task from the panel. Only `title` is required;
/// every field in [`NewTaskFields`] is optional and omitted from the body when empty. Same user
/// Cognito JWT as the reads; the backend gates on the caller's per-project role (and, for an
/// assignee, that they belong to the project).
pub async fn create_task(
    client: &reqwest::Client,
    ingest_url: &str,
    id_token: &str,
    project_id: &str,
    title: &str,
    fields: NewTaskFields<'_>,
) -> Result<TaskDto, String> {
    let url = format!(
        "{}/v1/projects/{}/tasks",
        ingest_url.trim_end_matches('/'),
        project_id
    );
    let mut body = serde_json::json!({ "title": title, "description": fields.description });
    if !fields.assignee_id.is_empty() {
        body["assignee_ids"] = serde_json::json!([fields.assignee_id]);
    }
    if !fields.due.is_empty() {
        body["due"] = serde_json::json!(fields.due);
    }
    if !fields.priority.is_empty() {
        body["priority"] = serde_json::json!(fields.priority);
    }
    let resp = client
        .post(url)
        .bearer_auth(id_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("create_task:network:{e}"))?;
    let status = resp.status();
    if status.as_u16() == 401 {
        return Err("auth:expired".into());
    }
    let text = resp
        .text()
        .await
        .map_err(|e| format!("create_task:read:{e}"))?;
    if !status.is_success() {
        return Err(format!("create_task:status:{}:{text}", status.as_u16()));
    }
    parse_task(&text)
}

/// Unwrap `{ "data": { ...TaskView } }` into the picker's `TaskDto` (extra fields serde-dropped).
fn parse_task(text: &str) -> Result<TaskDto, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("create_task:parse:{e}"))?;
    let inner = value.get("data").cloned().unwrap_or(value);
    serde_json::from_value(inner).map_err(|e| format!("create_task:parse:{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_enveloped_tasks_with_titles() {
        let json = r#"{"data":{"tasks":[
            {"id":"k-1","title":"Tray redesign","project_id":"p1","status":"todo"},
            {"id":"k-2","title":"Ingest retry","project_id":"p1","status":"in_progress","due":"2026-08-01"}
        ]}}"#;
        let ts = parse_tasks(json).unwrap();
        assert_eq!(ts.len(), 2);
        assert_eq!(ts[0].title, "Tray redesign");
        assert_eq!(ts[1].project_id, "p1");
    }

    /// The picker groups on this, so it has to survive the parse — and default the safe way when a
    /// backend that predates the field answers. Every task reading "up for grabs" would be a far
    /// louder wrong than none of them doing so.
    #[test]
    fn unclaimed_tasks_are_flagged_and_default_to_claimed() {
        let json = r#"{"data":{"tasks":[
            {"id":"k-1","title":"Mine","project_id":"p1","unassigned":false},
            {"id":"k-2","title":"Nobody's","project_id":"p1","unassigned":true},
            {"id":"k-3","title":"Old backend","project_id":"p1"}
        ]}}"#;
        let ts = parse_tasks(json).unwrap();
        assert!(!ts[0].unassigned);
        assert!(ts[1].unassigned);
        assert!(
            !ts[2].unassigned,
            "an absent flag must not read as unclaimed"
        );
    }

    #[test]
    fn missing_tasks_field_is_an_error() {
        assert!(parse_tasks(r#"{"data":{}}"#).is_err());
    }
}
