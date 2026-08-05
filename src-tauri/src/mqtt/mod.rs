//! MQTT downlink — the push rail (backend/docs/MQTT-MIGRATION.md Phase 3). One background tokio
//! task holds a mutual-TLS connection to AWS IoT Core using the per-install X.509 credential from
//! `POST /v1/agent/enroll`, subscribes the device's `cmd` topic, and turns server commands into
//! local actions (`config_changed` → pull `/v1/agent/config` immediately instead of waiting for the
//! next poll/cycle; `capture_now` → the admin-triggered on-demand screenshot, guarded in
//! [`capture`]). Presence rides the same connection: `{"online":true}` on connect, a
//! Last Will `{"online":false}` on unclean disconnect, and a best-effort clean `{"online":false}`
//! on app exit (`shutdown`).
//!
//! **This rail carries signals, never data.** Batches stay on HTTPS (`POST /v1/agent/batch`); the
//! HTTP config poller and the batch-ack version check remain the backstop, so a dropped MQTT
//! connection degrades to today's polling behaviour, never to silence.
//!
//! The credential is **device-level, per-install** (like `agent_id`) — it survives sign-out and
//! account switches, so `reset_for_account_switch` deliberately leaves it (and this connection)
//! alone. Enrollment itself needs a signed-in user (the enroll route is user-JWT-authed), so an
//! unenrolled agent waits for a session and enrolls once.

pub mod capture;

use std::time::Duration;

use rumqttc::{AsyncClient, Event, LastWill, MqttOptions, Packet, QoS, Transport};
use serde::Deserialize;
use tauri::Manager;

use crate::api::enroll::DeviceCredential;
use crate::api::{client, config, enroll};
use crate::auth::token_store;
use crate::state::AppState;

/// Keyring key for the persisted [`DeviceCredential`] (JSON), beside the refresh token. The private
/// key inside it is returned by the server exactly once, so this blob is the only copy.
const CREDENTIAL_KEY: &str = "device_credential";

/// The device credential, read from the keyring **once per process** and then served from memory.
/// Each keyring read is a macOS Keychain access prompt, and `ensure_credential` runs on every
/// (re)connection attempt — so without this cache a dropped connection re-prompts the user on each
/// retry. Populated on first load or enrollment; cleared by [`clear_credential`].
static CREDENTIAL_CACHE: std::sync::Mutex<Option<DeviceCredential>> = std::sync::Mutex::new(None);

/// Amazon Root CA 1 — the public root that signs AWS IoT Core's ATS server certificates
/// (https://www.amazontrust.com/repository/AmazonRootCA1.pem). Embedded so the agent needs no
/// system trust store for the broker connection.
const AMAZON_ROOT_CA1: &str = "-----BEGIN CERTIFICATE-----
MIIDQTCCAimgAwIBAgITBmyfz5m/jAo54vB4ikPmljZbyjANBgkqhkiG9w0BAQsF
ADA5MQswCQYDVQQGEwJVUzEPMA0GA1UEChMGQW1hem9uMRkwFwYDVQQDExBBbWF6
b24gUm9vdCBDQSAxMB4XDTE1MDUyNjAwMDAwMFoXDTM4MDExNzAwMDAwMFowOTEL
MAkGA1UEBhMCVVMxDzANBgNVBAoTBkFtYXpvbjEZMBcGA1UEAxMQQW1hem9uIFJv
b3QgQ0EgMTCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBALJ4gHHKeNXj
ca9HgFB0fW7Y14h29Jlo91ghYPl0hAEvrAIthtOgQ3pOsqTQNroBvo3bSMgHFzZM
9O6II8c+6zf1tRn4SWiw3te5djgdYZ6k/oI2peVKVuRF4fn9tBb6dNqcmzU5L/qw
IFAGbHrQgLKm+a/sRxmPUDgH3KKHOVj4utWp+UhnMJbulHheb4mjUcAwhmahRWa6
VOujw5H5SNz/0egwLX0tdHA114gk957EWW67c4cX8jJGKLhD+rcdqsq08p8kDi1L
93FcXmn/6pUCyziKrlA4b9v7LWIbxcceVOF34GfID5yHI9Y/QCB/IIDEgEw+OyQm
jgSubJrIqg0CAwEAAaNCMEAwDwYDVR0TAQH/BAUwAwEB/zAOBgNVHQ8BAf8EBAMC
AYYwHQYDVR0OBBYEFIQYzIU07LwMlJQuCFmcx7IQTgoIMA0GCSqGSIb3DQEBCwUA
A4IBAQCY8jdaQZChGsV2USggNiMOruYou6r4lK5IpDB/G/wkjUu0yKGX9rbxenDI
U5PMCCjjmCXPI6T53iHTfIUJrU6adTrCC2qJeHZERxhlbI1Bjjt/msv0tadQ1wUs
N+gDS63pYaACbvXy8MWy7Vu33PqUXHeeE6V/Uq2V8viTO96LXFvKWlJbYK8U90vv
o/ufQJVtMVT8QtPHRh8jrdkPSHCa2XV4cdFyQzR1bldZwgJcJmApzyMZFo6IQ6XU
5MsI+yMRQ+hDKXJioaldXgjUkK642M4UwtBV8ob2xJNDd2ZhwLnoQdeXeGADbkpy
rqXRfboQnoZsG4q5WTP468SQvvG5
-----END CERTIFICATE-----
";

/// Presence payloads — pinned by tests; the fleet consumer (IoT rule) matches on `online`.
const ONLINE_PAYLOAD: &[u8] = br#"{"online":true}"#;
const OFFLINE_PAYLOAD: &[u8] = br#"{"online":false}"#;

/// Ack published to `evt_topic` after a `config_changed` command was applied.
const ACK_CONFIG_CHANGED: &[u8] = br#"{"kind":"ack","cmd":"config_changed"}"#;

/// How often an **unenrolled** agent re-checks for a session to enroll under. Once enrolled this
/// poll never runs again (the credential loads from the keyring).
const ENROLL_RETRY_SECS: u64 = 15;

/// Reconnect backoff: capped exponential, reset on a successful CONNACK.
const BACKOFF_INITIAL_SECS: u64 = 5;
const BACKOFF_MAX_SECS: u64 = 300;

/// The live client + the topic the exit hook publishes the clean offline to. Held in `AppState` so
/// the (synchronous) `ExitRequested` handler can reach it.
pub struct MqttHandle {
    client: AsyncClient,
    status_topic: String,
}

/// Commands the backend publishes to `cmd_topic`. Internally tagged on `kind`; unknown kinds fold
/// into `Unknown` so a newer backend never breaks an older agent (forward-compatible).
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Command {
    /// "Pull `/v1/agent/config` now" — the admin changed policy; don't wait for the 60 s poller.
    ConfigChanged,
    /// "Take one screenshot of this employee's screen, now, and PUT it here." The most invasive
    /// command in the product — every guard lives in [`capture`], and every outcome is acked.
    CaptureNow {
        screenshot_id: String,
        upload_url: String,
        /// The user who asked. **Echoed verbatim** in the ack: it is the server's whole routing
        /// mechanism for the answer (`fleet::features::agent_events`).
        #[serde(default)]
        requested_by: String,
    },
    #[serde(other)]
    Unknown,
}

/// Spawn the MQTT supervisor on Tauri's async (tokio) runtime: enroll if needed, connect, and keep
/// reconnecting with capped exponential backoff for the life of the process.
pub fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let http = client::api_client();
        let mut backoff = Duration::from_secs(BACKOFF_INITIAL_SECS);
        loop {
            let Some(cred) = ensure_credential(&app, &http).await else {
                // Not signed in yet (or enroll failed) — try again shortly. Cheap: keyring + one
                // conditional network call, and only until enrollment succeeds once.
                tokio::time::sleep(Duration::from_secs(ENROLL_RETRY_SECS)).await;
                continue;
            };
            match run_connection(&app, &http, &cred, &mut backoff).await {
                Ok(()) => {
                    // Clean client-initiated disconnect (app exit) — stop supervising.
                    tracing::info!("mqtt: clean disconnect, supervisor exiting");
                    return;
                }
                Err(e) => {
                    tracing::warn!("mqtt: connection lost ({e}); reconnecting in {backoff:?}");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(BACKOFF_MAX_SECS));
                }
            }
        }
    });
}

/// Best-effort clean presence on app exit: queue `{"online":false}` + DISCONNECT on the still-running
/// event loop. If the process dies before they flush, the broker's Last Will delivers the same
/// offline payload anyway — this only makes the common case immediate instead of keepalive-delayed.
pub fn shutdown(state: &AppState) {
    let Some(handle) = state.mqtt.lock().unwrap().take() else {
        return;
    };
    if let Err(e) = handle.client.try_publish(
        &handle.status_topic,
        QoS::AtLeastOnce,
        false,
        OFFLINE_PAYLOAD.to_vec(),
    ) {
        tracing::debug!("mqtt: offline publish on exit not queued: {e}");
    }
    if let Err(e) = handle.client.try_disconnect() {
        tracing::debug!("mqtt: disconnect on exit not queued: {e}");
    }
}

/// The device credential: from the keyring if present, else enroll once under the signed-in user's
/// JWT and persist. `None` = can't enroll yet (signed out / network) — caller retries.
async fn ensure_credential(
    app: &tauri::AppHandle,
    http: &reqwest::Client,
) -> Option<DeviceCredential> {
    // Serve from memory first: each keyring read is a macOS Keychain prompt, and this runs on every
    // (re)connection attempt — cache it so the user is asked at most once per process, not per retry.
    if let Some(cred) = CREDENTIAL_CACHE.lock().unwrap().clone() {
        return Some(cred);
    }
    if let Some(cred) = load_credential() {
        *CREDENTIAL_CACHE.lock().unwrap() = Some(cred.clone());
        return Some(cred);
    }

    let state = app.state::<AppState>();
    // Enrollment is user-JWT-authed (same as /v1/agent/batch) — wait for a session.
    let id_token = state.auth.id_token().await?;
    let ingest_url = state.auth.config().ingest_url.clone();
    let agent_id = state.outbox.lock().unwrap().agent_id().to_string();

    match enroll::enroll(http, &ingest_url, &id_token, &agent_id).await {
        Ok(cred) => {
            // The private key exists nowhere else — persist BEFORE first use, and say so loudly if
            // that fails (the connection still works this run; the next launch re-enrolls).
            if let Err(e) = store_credential(&cred) {
                tracing::error!(
                    "mqtt: could not persist the device credential ({e}) — will re-enroll next launch"
                );
            } else {
                tracing::info!(thing = %cred.thing_name, "mqtt: enrolled device");
            }
            *CREDENTIAL_CACHE.lock().unwrap() = Some(cred.clone());
            Some(cred)
        }
        Err(e) => {
            tracing::warn!("mqtt: enrollment failed ({e}) — will retry");
            None
        }
    }
}

/// One connection lifetime: connect (mutual TLS, client id = thing name, LWT on `status`), announce
/// presence + subscribe `cmd` on CONNACK, dispatch commands until the connection errors.
/// `Ok(())` only on a clean client-initiated disconnect (app exit).
async fn run_connection(
    app: &tauri::AppHandle,
    http: &reqwest::Client,
    cred: &DeviceCredential,
    backoff: &mut Duration,
) -> Result<(), String> {
    // Defensive: the endpoint is a bare ATS hostname, but strip a scheme if one ever appears.
    let host = cred
        .iot_endpoint
        .trim_start_matches("mqtts://")
        .trim_start_matches("https://")
        .trim_end_matches('/');

    // Client id MUST equal the Thing name — the IoT policy is scoped by
    // `${iot:Connection.Thing.ThingName}`, so anything else is refused at CONNECT.
    let mut opts = MqttOptions::new(cred.thing_name.clone(), host, 8883);
    opts.set_keep_alive(Duration::from_secs(60));
    opts.set_last_will(LastWill::new(
        cred.status_topic.clone(),
        OFFLINE_PAYLOAD.to_vec(),
        QoS::AtLeastOnce,
        false,
    ));
    opts.set_transport(Transport::tls(
        AMAZON_ROOT_CA1.as_bytes().to_vec(),
        Some((
            cred.certificate_pem.clone().into_bytes(),
            cred.private_key.clone().into_bytes(),
        )),
        None,
    ));

    let (mqtt, mut eventloop) = AsyncClient::new(opts, 16);

    // Expose the client to the exit hook for the clean-offline publish.
    {
        let state = app.state::<AppState>();
        *state.mqtt.lock().unwrap() = Some(MqttHandle {
            client: mqtt.clone(),
            status_topic: cred.status_topic.clone(),
        });
    }

    let result = loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                tracing::info!(thing = %cred.thing_name, "mqtt: connected");
                *backoff = Duration::from_secs(BACKOFF_INITIAL_SECS);
                if let Err(e) = mqtt
                    .publish(
                        &cred.status_topic,
                        QoS::AtLeastOnce,
                        false,
                        ONLINE_PAYLOAD.to_vec(),
                    )
                    .await
                {
                    break Err(format!("mqtt:publish:{e}"));
                }
                if let Err(e) = mqtt.subscribe(&cred.cmd_topic, QoS::AtLeastOnce).await {
                    break Err(format!("mqtt:subscribe:{e}"));
                }
            }
            Ok(Event::Incoming(Packet::Publish(p))) => {
                if p.topic == cred.cmd_topic {
                    handle_command(app, http, &mqtt, cred, &p.payload).await;
                } else {
                    tracing::debug!(topic = %p.topic, "mqtt: publish on unexpected topic ignored");
                }
            }
            Ok(Event::Outgoing(rumqttc::Outgoing::Disconnect)) => {
                // Client-initiated (shutdown()) — a clean exit, not a failure.
                break Ok(());
            }
            Ok(_) => {}
            Err(e) => break Err(format!("mqtt:connection:{e}")),
        }
    };

    // Drop the exit-hook handle for this dead connection (unless shutdown already took it).
    app.state::<AppState>().mqtt.lock().unwrap().take();
    result
}

/// Dispatch one command from `cmd_topic`. Unknown/unparseable payloads are logged and ignored so a
/// newer backend can add kinds without breaking deployed agents.
async fn handle_command(
    app: &tauri::AppHandle,
    http: &reqwest::Client,
    mqtt: &AsyncClient,
    cred: &DeviceCredential,
    payload: &[u8],
) {
    match serde_json::from_slice::<Command>(payload) {
        Ok(Command::ConfigChanged) => {
            tracing::info!("mqtt: config_changed — pulling config now");
            pull_and_apply_config(app, http).await;
            if let Err(e) = mqtt
                .publish(
                    &cred.evt_topic,
                    QoS::AtLeastOnce,
                    false,
                    ACK_CONFIG_CHANGED.to_vec(),
                )
                .await
            {
                tracing::warn!("mqtt: config_changed ack not published: {e}");
            }
        }
        Ok(Command::CaptureNow {
            screenshot_id,
            upload_url,
            requested_by,
        }) => {
            tracing::info!(id = %screenshot_id, %requested_by, "mqtt: capture_now requested");
            // Off the event loop: a grab + encode + S3 PUT can take seconds (the upload client
            // allows up to 180 s), and the loop must keep polling or the connection misses its
            // keepalive and drops mid-capture. The ack is published from the spawned task on the
            // same (cloneable) client.
            let app = app.clone();
            let mqtt = mqtt.clone();
            let evt_topic = cred.evt_topic.clone();
            tauri::async_runtime::spawn(async move {
                let ack = capture::handle(&app, &screenshot_id, &upload_url, &requested_by).await;
                if let Err(e) = mqtt.publish(&evt_topic, QoS::AtLeastOnce, false, ack).await {
                    // The requester is left waiting, but the local disclosure and the capture
                    // decision itself already happened — nothing is silently *done*, only silently
                    // unanswered, and the server audits from its own request record.
                    tracing::warn!("mqtt: capture_now result not published: {e}");
                }
            });
        }
        Ok(Command::Unknown) => {
            tracing::info!("mqtt: unknown command kind ignored (forward-compatible)");
        }
        Err(e) => tracing::warn!("mqtt: unparseable command payload ignored: {e}"),
    }
}

/// The same conditional (ETag) pull-and-apply the 60 s poller and the batch-ack path do — triggered
/// by the server's push instead of a clock. Auth-gated like every API call; a failure is logged and
/// the polling rails remain the backstop.
async fn pull_and_apply_config(app: &tauri::AppHandle, http: &reqwest::Client) {
    let state = app.state::<AppState>();
    let Some(id_token) = state.auth.id_token().await else {
        tracing::info!("mqtt: config pull skipped (signed out)");
        return;
    };
    let ingest_url = state.auth.config().ingest_url.clone();
    let etag = state.config.lock().unwrap().etag();
    match config::pull_config(http, &ingest_url, &id_token, etag.as_deref()).await {
        Ok(config::ConfigPull::Fresh { config, etag }) => {
            let v = config.version;
            state.config.lock().unwrap().apply(*config, etag);
            tracing::info!("mqtt: config applied (version {v})");
        }
        Ok(config::ConfigPull::NotModified) => {}
        Err(e) => tracing::warn!("mqtt: config pull failed: {e}"),
    }
}

/// Load the persisted device credential from the keyring, or `None` if absent/corrupt (→ re-enroll).
fn load_credential() -> Option<DeviceCredential> {
    let blob = token_store::load(CREDENTIAL_KEY)?;
    match serde_json::from_slice(&blob) {
        Ok(cred) => Some(cred),
        Err(e) => {
            tracing::warn!("mqtt: stored device credential unreadable ({e}) — re-enrolling");
            None
        }
    }
}

/// Persist the device credential (JSON, chunked) in the OS keyring beside the tokens.
fn store_credential(cred: &DeviceCredential) -> keyring::Result<()> {
    let blob = serde_json::to_vec(cred).expect("credential serializes");
    token_store::store(CREDENTIAL_KEY, &blob)
}

/// Remove the persisted credential (no caller yet; the revocation path is server-side — cert
/// deactivation on `workforce.employee_deactivated` — but a local wipe belongs beside store/load).
fn clear_credential() {
    let _ = token_store::clear(CREDENTIAL_KEY);
    *CREDENTIAL_CACHE.lock().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config_changed_command() {
        let c: Command = serde_json::from_slice(br#"{"kind":"config_changed"}"#).unwrap();
        assert!(matches!(c, Command::ConfigChanged));
    }

    /// Unknown kinds must parse (to `Unknown`), not error — a newer backend adding a command kind
    /// must never make a deployed agent log parse failures.
    #[test]
    fn unknown_command_kind_is_tolerated() {
        let c: Command =
            serde_json::from_slice(br#"{"kind":"reboot_please","extra":"field"}"#).unwrap();
        assert!(matches!(c, Command::Unknown));
    }

    /// The exact payload `fleet::features::capture_now` publishes.
    #[test]
    fn parses_capture_now_command() {
        let raw = br#"{"kind":"capture_now","screenshot_id":"01J0","upload_url":"https://b.s3.ap-south-1.amazonaws.com/k?sig=x","requested_by":"u7"}"#;
        let c: Command = serde_json::from_slice(raw).unwrap();
        match c {
            Command::CaptureNow {
                screenshot_id,
                upload_url,
                requested_by,
            } => {
                assert_eq!(screenshot_id, "01J0");
                assert!(upload_url.starts_with("https://"));
                assert_eq!(requested_by, "u7");
            }
            other => panic!("expected CaptureNow, got {other:?}"),
        }
    }

    /// An older backend that omits `requested_by` must still capture — the ack simply has nobody to
    /// route back to (the server drops it), which is better than refusing the command outright.
    #[test]
    fn capture_now_without_a_requester_still_parses() {
        let raw = br#"{"kind":"capture_now","screenshot_id":"s","upload_url":"https://x.amazonaws.com/k"}"#;
        let c: Command = serde_json::from_slice(raw).unwrap();
        assert!(
            matches!(c, Command::CaptureNow { ref requested_by, .. } if requested_by.is_empty())
        );
    }

    #[test]
    fn garbage_payload_is_an_error_not_a_panic() {
        assert!(serde_json::from_slice::<Command>(b"not json").is_err());
        assert!(serde_json::from_slice::<Command>(br#"{"no_kind":true}"#).is_err());
    }

    /// The presence + ack payloads are wire contracts (the IoT rule / fleet consumer matches on
    /// them) — pin the exact JSON.
    #[test]
    fn status_and_ack_payloads_are_pinned() {
        let online: serde_json::Value = serde_json::from_slice(ONLINE_PAYLOAD).unwrap();
        assert_eq!(online, serde_json::json!({"online": true}));

        let offline: serde_json::Value = serde_json::from_slice(OFFLINE_PAYLOAD).unwrap();
        assert_eq!(offline, serde_json::json!({"online": false}));

        let ack: serde_json::Value = serde_json::from_slice(ACK_CONFIG_CHANGED).unwrap();
        assert_eq!(
            ack,
            serde_json::json!({"kind": "ack", "cmd": "config_changed"})
        );
    }

    /// The embedded root must be one well-formed PEM certificate block (rumqttc feeds it to
    /// rustls-pemfile; an empty parse would fail every connect with `NoValidCertInChain`).
    #[test]
    fn amazon_root_ca_is_a_pem_certificate() {
        let pem = AMAZON_ROOT_CA1.trim();
        assert!(pem.starts_with("-----BEGIN CERTIFICATE-----"));
        assert!(pem.ends_with("-----END CERTIFICATE-----"));
        let body: String = pem
            .lines()
            .filter(|l| !l.starts_with("-----"))
            .collect::<Vec<_>>()
            .join("");
        use base64::Engine;
        let der = base64::engine::general_purpose::STANDARD
            .decode(body)
            .expect("PEM body is valid base64");
        // DER SEQUENCE header — sanity that this is a certificate, not prose.
        assert_eq!(der[0], 0x30);
    }
}
