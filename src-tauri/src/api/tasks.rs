//! `GET /v1/me/tasks` — the caller's assigned tasks (across projects), so the panel's task picker
//! shows real tasks. Same user Cognito JWT as the batch rail. The agent needs `id`/`title`/
//! `project_id`; `status`/`due` are ignored (serde drops unknown fields).

use serde::{Deserialize, Serialize};

/// One task for the panel picker (subset of the backend `MyTaskRow`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskDto {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub project_id: String,
}

pub async fn fetch_tasks(
    client: &reqwest::Client,
    ingest_url: &str,
    id_token: &str,
) -> Result<Vec<TaskDto>, String> {
    let url = format!("{}/v1/me/tasks", ingest_url.trim_end_matches('/'));
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

    #[test]
    fn missing_tasks_field_is_an_error() {
        assert!(parse_tasks(r#"{"data":{}}"#).is_err());
    }
}
