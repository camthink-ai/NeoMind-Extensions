use crate::types::{HaEntity, HaStateResponse};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;
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

/// Run the WebSocket event loop that maintains a persistent connection to HA,
/// performs the auth handshake, subscribes to state_changed events, and
/// continuously updates the shared entity map.
///
/// This function is designed to be spawned on the host Tokio runtime via
/// `handle.spawn()`.
pub async fn run_ws_loop(
    ha_url: String,
    token: String,
    entities: Arc<RwLock<HashMap<String, HaEntity>>>,
    running: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
) {
    let mut backoff_secs: u64 = 1;
    let max_backoff_secs: u64 = 60;

    while running.load(Ordering::SeqCst) {
        let ws_url = build_ws_url(&ha_url);

        match connect_and_listen(&ws_url, &token, &entities, &running).await {
            Ok(()) => {
                // Clean shutdown requested
                connected.store(false, Ordering::SeqCst);
                break;
            }
            Err(e) => {
                eprintln!("HA WebSocket disconnected ({}), reconnecting in {}s", e, backoff_secs);
                connected.store(false, Ordering::SeqCst);

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
        }
    }

    connected.store(false, Ordering::SeqCst);
}

/// Connect to HA WebSocket, authenticate, subscribe to state_changed,
/// and read events until an error occurs or shutdown is requested.
async fn connect_and_listen(
    ws_url: &str,
    token: &str,
    entities: &Arc<RwLock<HashMap<String, HaEntity>>>,
    running: &AtomicBool,
) -> Result<(), String> {
    // Connect
    let (mut ws_stream, _response) = tokio_tungstenite::connect_async(ws_url)
        .await
        .map_err(|e| format!("WebSocket connect failed: {}", e))?;

    // Step 1: Read auth_required message from server
    let msg = ws_stream
        .next()
        .await
        .ok_or_else(|| "WebSocket closed before auth".to_string())?
        .map_err(|e| format!("WebSocket read error during auth: {}", e))?;

    let auth_required: serde_json::Value = parse_ws_text(&msg)?;
    if auth_required
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        != "auth_required"
    {
        return Err("Expected auth_required message from HA".to_string());
    }

    // Step 2: Send auth
    let auth_msg = serde_json::json!({
        "type": "auth",
        "access_token": token
    });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .map_err(|e| format!("Failed to send auth: {}", e))?;

    // Step 3: Read auth response
    let msg = ws_stream
        .next()
        .await
        .ok_or_else(|| "WebSocket closed during auth response".to_string())?
        .map_err(|e| format!("WebSocket read error during auth response: {}", e))?;

    let auth_result: serde_json::Value = parse_ws_text(&msg)?;
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
            return Err(format!("HA auth failed: {}", message));
        }
        other => {
            return Err(format!("Unexpected auth response type: {}", other));
        }
    }

    // Step 4: Subscribe to state_changed events
    let subscribe_msg = serde_json::json!({
        "id": 1,
        "type": "subscribe_events",
        "event_type": "state_changed"
    });
    ws_stream
        .send(Message::Text(subscribe_msg.to_string().into()))
        .await
        .map_err(|e| format!("Failed to subscribe: {}", e))?;

    // Read the subscription confirmation
    let msg = ws_stream
        .next()
        .await
        .ok_or_else(|| "WebSocket closed after subscribe".to_string())?
        .map_err(|e| format!("WebSocket read error after subscribe: {}", e))?;

    let conf: serde_json::Value = parse_ws_text(&msg)?;
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
        return Err(format!("Subscribe failed: {}", conf));
    }

    eprintln!("HA WebSocket connected and subscribed to state_changed");

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
                        handle_ws_message(&parsed, entities).await;
                    }
                }
            }
            Ok(Some(Err(e))) => {
                return Err(format!("WebSocket read error: {}", e));
            }
            Ok(None) => {
                return Err("WebSocket closed by server".to_string());
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

    Ok(())
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
async fn handle_ws_message(
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

    let mut ents = entities.write().await;
    ents.insert(entity_id, entity);
}
