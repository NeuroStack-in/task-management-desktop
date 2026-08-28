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

/// One subtask — a single level of breakdown under a task.
///
/// Real work, not a checklist tick: the timer runs against it. Created and ticked off here, in the
/// agent — the web app shows them read-only, which is why this is the only client that writes them.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubtaskDto {
    pub id: String,
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub title: String,
    /// `todo` | `in_progress` | `in_review` | `done` | `blocked`. Never `closed` — that means a lead
    /// signed a *task* off, and a subtask has no review step, so the server rejects it.
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub assignee_id: String,
}

impl SubtaskDto {
    /// Finished. Both terminal states count: `closed` cannot be set here but can arrive on a row, and
    /// a picker that ignored it would show signed-off work as still outstanding.
    pub fn is_done(&self) -> bool {
        self.status == "done" || self.status == "closed"
    }
}

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
    /// `todo` | `in_progress` | `in_review` | `done` | `closed` | `blocked`.
    ///
    /// The panel needs it to show whether the task is finished and to offer "mark done". Defaults
    /// to `todo` so a row from a backend that omits it renders as open rather than as nothing.
    #[serde(default)]
    pub status: String,
    /// This task's breakdown, from `?include_subtasks=true`.
    ///
    /// Empty is the normal case and means "no breakdown" — the picker then offers the task itself,
    /// exactly as it did before subtasks existed. `#[serde(default)]` so a backend that predates the
    /// field answers with an empty list rather than failing the whole picker.
    #[serde(default)]
    pub subtasks: Vec<SubtaskDto>,
}

pub async fn fetch_tasks(
    client: &reqwest::Client,
    ingest_url: &str,
    id_token: &str,
) -> Result<Vec<TaskDto>, String> {
    // `include_subtasks` costs one extra query **per project** server-side, never one per task —
    // so the whole project → task → subtask picker is one round trip rather than one per row.
    let url = format!(
        "{}/v1/me/tasks?include_unassigned=true&include_subtasks=true",
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

/// `POST /v1/projects/{project_id}/tasks/{task_id}/subtasks` — add one level of breakdown.
///
/// Only `title` is sent. The server defaults the status to `todo` and the assignee to **the
/// caller**, which is what an employee breaking down their own work wants — so there is nothing
/// else for the panel to ask for.
///
/// Any project member may do this, on any task in a project they belong to.
pub async fn create_subtask(
    client: &reqwest::Client,
    ingest_url: &str,
    id_token: &str,
    project_id: &str,
    task_id: &str,
    title: &str,
) -> Result<SubtaskDto, String> {
    let url = subtask_url(ingest_url, project_id, task_id, None);
    let resp = client
        .post(url)
        .bearer_auth(id_token)
        .json(&serde_json::json!({ "title": title }))
        .send()
        .await
        .map_err(|e| format!("create_subtask:network:{e}"))?;
    read_subtask(resp, "create_subtask").await
}

/// `PATCH …/subtasks/{subtask_id}` — tick one off, or move it back.
pub async fn set_subtask_status(
    client: &reqwest::Client,
    ingest_url: &str,
    id_token: &str,
    project_id: &str,
    task_id: &str,
    subtask_id: &str,
    status: &str,
) -> Result<SubtaskDto, String> {
    let url = subtask_url(ingest_url, project_id, task_id, Some(subtask_id));
    let resp = client
        .patch(url)
        .bearer_auth(id_token)
        .json(&serde_json::json!({ "status": status }))
        .send()
        .await
        .map_err(|e| format!("set_subtask_status:network:{e}"))?;
    read_subtask(resp, "set_subtask_status").await
}

/// Percent-free path join. Ids are server-minted ULIDs (`k-…`, `s-…`), so there is nothing to
/// escape — but they are still built in one place so the two callers cannot drift.
fn subtask_url(
    ingest_url: &str,
    project_id: &str,
    task_id: &str,
    subtask_id: Option<&str>,
) -> String {
    let base = format!(
        "{}/v1/projects/{}/tasks/{}/subtasks",
        ingest_url.trim_end_matches('/'),
        project_id,
        task_id
    );
    match subtask_id {
        Some(id) => format!("{base}/{id}"),
        None => base,
    }
}

/// Shared response handling for both writes.
///
/// A **403** is its own message rather than a raw status: it means the subtask belongs to someone
/// else and the caller is only a project Member. That is a rule the person can act on ("ask a lead"),
/// and printing `set_subtask_status:status:403:{...}` at them instead would be noise.
async fn read_subtask(resp: reqwest::Response, op: &str) -> Result<SubtaskDto, String> {
    let status = resp.status();
    if status.as_u16() == 401 {
        return Err("auth:expired".into());
    }
    let text = resp.text().await.map_err(|e| format!("{op}:read:{e}"))?;
    if status.as_u16() == 403 {
        return Err("subtask:not-yours".into());
    }
    if !status.is_success() {
        return Err(format!("{op}:status:{}:{text}", status.as_u16()));
    }
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{op}:parse:{e}"))?;
    let inner = value.get("data").cloned().unwrap_or(value);
    serde_json::from_value(inner).map_err(|e| format!("{op}:parse:{e}"))
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

    /// The picker reads the breakdown from the same call as the tasks. A task with none must come
    /// back with an empty list, not fail the parse — that is every task today.
    #[test]
    fn subtasks_ride_the_task_list_and_default_to_empty() {
        let json = r#"{"data":{"tasks":[
            {"id":"k-1","title":"AI integration","project_id":"p1","subtasks":[
                {"id":"s-1","task_id":"k-1","title":"Fine tuning","status":"done"},
                {"id":"s-2","task_id":"k-1","title":"AI chatbot","status":"todo"}
            ]},
            {"id":"k-2","title":"Plain task","project_id":"p1"}
        ]}}"#;
        let ts = parse_tasks(json).unwrap();
        assert_eq!(ts[0].subtasks.len(), 2);
        assert!(ts[0].subtasks[0].is_done());
        assert!(!ts[0].subtasks[1].is_done());
        assert!(
            ts[1].subtasks.is_empty(),
            "a task with no breakdown must parse, not fail"
        );
    }

    #[test]
    fn the_subtask_url_addresses_the_collection_and_one_row() {
        assert_eq!(
            subtask_url("https://api/", "p1", "k1", None),
            "https://api/v1/projects/p1/tasks/k1/subtasks"
        );
        assert_eq!(
            subtask_url("https://api", "p1", "k1", Some("s1")),
            "https://api/v1/projects/p1/tasks/k1/subtasks/s1"
        );
    }
}
