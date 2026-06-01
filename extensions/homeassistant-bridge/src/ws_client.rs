use crate::rest_client::HaRestClient;
use crate::types::{HaEntity, HaStateResponse};
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_tungstenite::tungstenite::Message;

/// Convert an HA HTTP URL to its WebSocket equivalent.
/// e.g. "http://192.168.1.10:8123" -> "ws://192.168.1.10:8123/api/websocket"
fn build_ws_url(ha_url: &str) -> String {
    let url = ha_url.trim_end_matches('/');
    let ws_base = if url.starts_with("https://") {
        url.replacen("https://", "wss://", 1)
    } else if url.starts_with("http://") {
        url.replacen("http://", "ws://", 1)
    } else {
        format!("ws://{}", url)
    };
    ws_base + "/api/websocket"
}

/// Distinguishes between transient disconnects and permanent auth failures.
enum WsDisconnect {
    /// Clean shutdown requested via `running` flag.
    CleanShutdown,
    /// Transient network error; safe to retry.
    Transient(String),
    /// Permanent auth failure; bad token that won't change between retries.
    AuthFailed(String),
}

/// Run the WebSocket event loop that maintains a persistent connection to HA,
/// performs the auth handshake, subscribes to state_changed events, and
/// continuously updates the shared entity map.
///
/// This function is designed to be spawned on the host Tokio runtime via
/// `handle.spawn()`.
pub async fn run_ws_loop(
    ha_url: String,
    token: String,
    domains: Vec<String>,
    entities: Arc<RwLock<HashMap<String, HaEntity>>>,
    rest_client: Arc<RwLock<Option<HaRestClient>>>,
    running: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
) {
    let mut backoff_secs: u64 = 1;
    let max_backoff_secs: u64 = 60;
    let mut consecutive_auth_failures: u32 = 0;
    let max_auth_retries: u32 = 5;

    while running.load(Ordering::SeqCst) {
        let ws_url = build_ws_url(&ha_url);

        match connect_and_listen(&ws_url, &token, &domains, &entities, &running).await {
            WsDisconnect::CleanShutdown => {
                connected.store(false, Ordering::SeqCst);
                break;
            }
            WsDisconnect::Transient(e) => {
                // Connection was established (or could be established) — reset backoff
                // since the issue is likely transient (network blip, server restart).
                backoff_secs = 1;
                consecutive_auth_failures = 0;

                eprintln!("[ha-bridge] WebSocket disconnected ({}), reconnecting in {}s", e, backoff_secs);
                connected.store(false, Ordering::SeqCst);

                // REST resync on reconnection: fetch full state to avoid missing updates
                // that happened while the WebSocket was disconnected.
                do_rest_resync(&rest_client, &domains, &entities);

                // Wait with backoff, but check running flag periodically
                let wait_steps = backoff_secs;
                for _ in 0..wait_steps {
                    if !running.load(Ordering::SeqCst) {
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }

                backoff_secs = (backoff_secs * 2).min(max_backoff_secs);
            }
            WsDisconnect::AuthFailed(e) => {
                consecutive_auth_failures += 1;
                connected.store(false, Ordering::SeqCst);

                if consecutive_auth_failures >= max_auth_retries {
                    eprintln!(
                        "[ha-bridge] WebSocket auth failed {} times, stopping retries: {}",
                        consecutive_auth_failures, e
                    );
                    eprintln!("[ha-bridge] Generate a new Long-Lived Access Token in HA (Profile > Security) and reconfigure the extension");
                    break;
                }

                eprintln!(
                    "[ha-bridge] WebSocket auth failed ({}/{}): {}. Retrying in {}s",
                    consecutive_auth_failures, max_auth_retries, e, backoff_secs
                );

                let wait_steps = backoff_secs;
                for _ in 0..wait_steps {
                    if !running.load(Ordering::SeqCst) {
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }

                backoff_secs = (backoff_secs * 2).min(max_backoff_secs);
            }
        }
    }

    connected.store(false, Ordering::SeqCst);
}

/// Perform a full REST resync of all entity states.
/// Called on WebSocket reconnection to catch up on missed state changes.
fn do_rest_resync(
    rest_client: &Arc<RwLock<Option<HaRestClient>>>,
    domains: &[String],
    entities: &Arc<RwLock<HashMap<String, HaEntity>>>,
) {
    // Fetch data while holding only the rest_client read lock,
    // then release it before acquiring the entities write lock
    // to avoid potential deadlock.
    let fetched = {
        let client_guard = rest_client.read();
        if let Some(ref client) = *client_guard {
            client.get_all_states(domains)
        } else {
            return;
        }
    };

    match fetched {
        Ok(fetched) => {
            let mut ent_map = entities.write();
            for e in fetched {
                ent_map.insert(e.entity_id.clone(), e);
            }
            eprintln!("[ha-bridge] REST resync completed, {} entities updated", ent_map.len());
        }
        Err(e) => {
            eprintln!("[ha-bridge] REST resync failed: {}", e);
        }
    }
}

/// Connect to HA WebSocket, authenticate, subscribe to state_changed,
/// and read events until an error occurs or shutdown is requested.
async fn connect_and_listen(
    ws_url: &str,
    token: &str,
    _domains: &[String],
    entities: &Arc<RwLock<HashMap<String, HaEntity>>>,
    running: &AtomicBool,
) -> WsDisconnect {
    // Connect
    let (mut ws_stream, _response) = match tokio_tungstenite::connect_async(ws_url).await {
        Ok(s) => s,
        Err(e) => return WsDisconnect::Transient(format!("WebSocket connect failed: {}", e)),
    };

    // Step 1: Read auth_required message from server
    let msg = match ws_stream.next().await {
        Some(Ok(m)) => m,
        Some(Err(e)) => return WsDisconnect::Transient(format!("WebSocket read error during auth: {}", e)),
        None => return WsDisconnect::Transient("WebSocket closed before auth".to_string()),
    };

    let auth_required: serde_json::Value = match parse_ws_text(&msg) {
        Ok(v) => v,
        Err(e) => return WsDisconnect::Transient(e),
    };
    if auth_required
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        != "auth_required"
    {
        return WsDisconnect::Transient("Expected auth_required message from HA".to_string());
    }

    // Step 2: Send auth
    let auth_msg = serde_json::json!({
        "type": "auth",
        "access_token": token
    });
    if let Err(e) = ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
    {
        return WsDisconnect::Transient(format!("Failed to send auth: {}", e));
    }

    // Step 3: Read auth response
    let msg = match ws_stream.next().await {
        Some(Ok(m)) => m,
        Some(Err(e)) => return WsDisconnect::Transient(format!("WebSocket read error during auth response: {}", e)),
        None => return WsDisconnect::Transient("WebSocket closed during auth response".to_string()),
    };

    let auth_result: serde_json::Value = match parse_ws_text(&msg) {
        Ok(v) => v,
        Err(e) => return WsDisconnect::Transient(e),
    };
    match auth_result
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
    {
        "auth_ok" => {}
        "auth_invalid" => {
            let message = auth_result
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            return WsDisconnect::AuthFailed(message.to_string());
        }
        other => {
            return WsDisconnect::Transient(format!("Unexpected auth response type: {}", other));
        }
    }

    // Step 4: Subscribe to state_changed events
    let subscribe_msg = serde_json::json!({
        "id": 1,
        "type": "subscribe_events",
        "event_type": "state_changed"
    });
    if let Err(e) = ws_stream
        .send(Message::Text(subscribe_msg.to_string().into()))
        .await
    {
        return WsDisconnect::Transient(format!("Failed to subscribe: {}", e));
    }

    // Read the subscription confirmation
    let msg = match ws_stream.next().await {
        Some(Ok(m)) => m,
        Some(Err(e)) => return WsDisconnect::Transient(format!("WebSocket read error after subscribe: {}", e)),
        None => return WsDisconnect::Transient("WebSocket closed after subscribe".to_string()),
    };

    let conf: serde_json::Value = match parse_ws_text(&msg) {
        Ok(v) => v,
        Err(e) => return WsDisconnect::Transient(e),
    };
    if conf
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        != "result"
        || conf
            .get("success")
            .and_then(|v| v.as_bool())
        != Some(true)
    {
        return WsDisconnect::Transient(format!("Subscribe failed: {}", conf));
    }

    eprintln!("[ha-bridge] WebSocket connected and subscribed to state_changed");

    // Step 5: Read events in a loop
    while running.load(Ordering::SeqCst) {
        match tokio::time::timeout(
            std::time::Duration::from_secs(120),
            ws_stream.next(),
        )
        .await
        {
            Ok(Some(Ok(msg))) => {
                if let Ok(text) = msg.into_text() {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                        handle_ws_message(&parsed, entities);
                    }
                }
            }
            Ok(Some(Err(e))) => {
                return WsDisconnect::Transient(format!("WebSocket read error: {}", e));
            }
            Ok(None) => {
                return WsDisconnect::Transient("WebSocket closed by server".to_string());
            }
            Err(_) => {
                // Timeout — check if still running, then continue
                // Send a ping to keep the connection alive
                let _ = ws_stream
                    .send(Message::Ping(vec![].into()))
                    .await;
            }
        }
    }

    // Clean close
    let _ = ws_stream
        .send(Message::Close(None))
        .await;

    WsDisconnect::CleanShutdown
}

/// Parse a WebSocket text message into JSON.
fn parse_ws_text(msg: &Message) -> Result<serde_json::Value, String> {
    match msg {
        Message::Text(text) => {
            serde_json::from_str(text).map_err(|e| format!("JSON parse error: {}", e))
        }
        _ => Err("Expected text message".to_string()),
    }
}

/// Handle a parsed WebSocket message. We only care about event messages
/// of type "state_changed".
fn handle_ws_message(
    msg: &serde_json::Value,
    entities: &Arc<RwLock<HashMap<String, HaEntity>>>,
) {
    // Only handle event messages
    if msg
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        != "event"
    {
        return;
    }

    let event_data = match msg.get("event") {
        Some(d) => d,
        None => return,
    };

    let event_type = event_data
        .get("event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if event_type != "state_changed" {
        return;
    }

    let data = match event_data.get("data") {
        Some(d) => d,
        None => return,
    };

    // Extract new_state
    let new_state = match data.get("new_state") {
        Some(s) if !s.is_null() => s,
        _ => return, // Entity was removed (new_state is null)
    };

    // Parse into HaStateResponse
    let state_resp: HaStateResponse = match serde_json::from_value(new_state.clone()) {
        Ok(s) => s,
        Err(_) => return,
    };

    let entity = state_resp.to_entity();
    let entity_id = entity.entity_id.clone();

    let mut ents = entities.write();
    ents.insert(entity_id, entity);
}
