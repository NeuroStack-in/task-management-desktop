//! `GET /v1/projects` — the caller's projects, so the panel's "Select project" control shows real
//! data instead of an empty list. Same user Cognito JWT the batch rail uses (the agent logs in as
//! the user). The agent needs only `id`/`name`/`billable`; the rest of the backend `ProjectRow` is
//! ignored (serde drops unknown fields).

use serde::{Deserialize, Serialize};

/// One project for the panel picker (subset of the backend `ProjectRow`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub billable: bool,
    #[serde(default)]
    pub status: String,
}

pub async fn fetch_projects(
    client: &reqwest::Client,
    ingest_url: &str,
    id_token: &str,
) -> Result<Vec<ProjectDto>, String> {
    let url = format!("{}/v1/projects", ingest_url.trim_end_matches('/'));
    let resp = client
        .get(url)
        .bearer_auth(id_token)
        .send()
        .await
        .map_err(|e| format!("projects:network:{e}"))?;
    let status = resp.status();
    if status.as_u16() == 401 {
        return Err("auth:expired".into());
    }
    let text = resp
        .text()
        .await
        .map_err(|e| format!("projects:read:{e}"))?;
    if !status.is_success() {
        return Err(format!("projects:status:{}:{text}", status.as_u16()));
    }
    parse_projects(&text)
}

/// Unwrap `{ "data": { "projects": [...] } }` (the backend `Envelope`), else a bare `{ "projects" }`.
fn parse_projects(text: &str) -> Result<Vec<ProjectDto>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("projects:parse:{e}"))?;
    let inner = value.get("data").cloned().unwrap_or(value);
    let list = inner
        .get("projects")
        .cloned()
        .ok_or("projects:parse:missing 'projects' field")?;
    serde_json::from_value(list).map_err(|e| format!("projects:parse:{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_enveloped_and_bare_projects() {
        let enveloped = r#"{"data":{"projects":[
            {"id":"p1","name":"Platform","status":"active","billable":true,"manager_user_id":"u1"},
            {"id":"p2","name":"Internal","status":"active","billable":false,"manager_user_id":"u1"}
        ]}}"#;
        let ps = parse_projects(enveloped).unwrap();
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0].id, "p1");
        assert_eq!(ps[0].name, "Platform");
        assert!(ps[0].billable);

        let bare = r#"{"projects":[{"id":"p9","name":"Acme"}]}"#;
        let ps = parse_projects(bare).unwrap();
        assert_eq!(ps[0].name, "Acme");
        assert!(!ps[0].billable); // defaulted
    }

    #[test]
    fn missing_projects_field_is_an_error_not_empty() {
        assert!(parse_projects(r#"{"data":{}}"#).is_err());
    }
}
