//! Backend transport — **M2**. The sole network egress: drain the outbox oldest-first →
//! `POST /v1/agent/batch` with the Cognito **ID token** → on `BatchAck`: `prune_to(watermark_seq)`,
//! compare `config_version` → ETag config pull → PUT screenshot bytes to `upload_urls` (**host-pinned**:
//! reject unless https + `amazonaws.com`). Two `reqwest` clients: 30 s API, 180 s upload (BUILD-PLAN §4).
//!
//! Live dev: `https://oqlla6l5oc.execute-api.ap-south-1.amazonaws.com`.

// TODO(M2): client.rs (reqwest builders) · batch.rs (POST /v1/agent/batch) ·
//           config.rs (GET /v1/agent/config, ETag) · tasks.rs (GET /v1/agent/tasks, M3a).
