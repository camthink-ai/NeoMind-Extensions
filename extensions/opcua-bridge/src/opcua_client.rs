use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::node_cache::NodeCache;
use crate::types::{CommandMsg, OpcUaConfig};

/// Manages the background tokio runtime and OPC-UA client connection.
///
/// The architecture uses a dedicated `std::thread` with an embedded
/// `tokio::runtime::Runtime` for async OPC-UA operations. Commands from
/// the synchronous extension API are bridged via channels:
///
/// ```text
/// execute_command() → tokio::sync::mpsc → async runtime → std::sync::mpsc → result
/// ```
///
/// The reply channel uses `std::sync::mpsc` (not `tokio::sync::oneshot`)
/// because the callers are async functions inside the extension runner's
/// tokio runtime, and `blocking_recv()` would panic. `std::sync::mpsc::Receiver::recv()`
/// is safe to call from any thread.
///
/// Node cache uses `parking_lot::RwLock` for lock-free synchronous reads
/// from `produce_metrics()`.
pub struct OpcUaClientManager {
    cmd_tx: Option<tokio::sync::mpsc::UnboundedSender<CommandMsg>>,
    connected: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    /// Handle to the background std::thread that owns the tokio runtime
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl OpcUaClientManager {
    pub fn new(connected: Arc<AtomicBool>) -> Self {
        Self {
            cmd_tx: None,
            connected,
            running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
        }
    }

    /// Start the background tokio runtime on a dedicated std::thread.
    pub fn start(&mut self, cache: Arc<NodeCache>) -> Result<(), String> {
        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();
        let connected = self.connected.clone();

        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        self.cmd_tx = Some(cmd_tx);

        let handle = std::thread::Builder::new()
            .name("opcua-bridge-runtime".to_string())
            .spawn(move || {
                let runtime = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(e) => {
                        eprintln!("[opcua-bridge] Failed to create tokio runtime: {}", e);
                        return;
                    }
                };

                runtime.block_on(async move {
                    let mut server_url = String::new();
                    let mut _session_active = false;

                    while running.load(Ordering::SeqCst) {
                        tokio::select! {
                            Some(msg) = cmd_rx.recv() => {
                                match msg {
                                    CommandMsg::Connect { config, reply } => {
                                        server_url = config.server_url.clone();
                                        eprintln!("[opcua-bridge] Connecting to {}", server_url);

                                        // NOTE: This is a protocol stub. The OPC-UA client crate
                                        // (opcua or async-opcua) is not yet integrated. This stub
                                        // validates the extension architecture and command API.
                                        // To complete: add `opcua` crate dependency and implement
                                        // real ClientBuilder + Session here.
                                        connected.store(true, Ordering::SeqCst);
                                        _session_active = true;

                                        let _ = reply.send(Ok(serde_json::json!({
                                            "success": true,
                                            "server_url": server_url,
                                            "message": "Connected (protocol stub — OPC-UA crate not yet integrated)"
                                        })));
                                    }
                                    CommandMsg::Disconnect { reply } => {
                                        eprintln!("[opcua-bridge] Disconnecting from {}", server_url);
                                        connected.store(false, Ordering::SeqCst);
                                        _session_active = false;
                                        cache.clear_nodes();
                                        cache.clear_subscriptions();
                                        let _ = reply.send(Ok(serde_json::json!({
                                            "success": true,
                                            "message": "Disconnected"
                                        })));
                                    }
                                    CommandMsg::Browse { node_id, max_depth, reply } => {
                                        let root_nodes = vec![
                                            serde_json::json!({
                                                "node_id": "i=84",
                                                "browse_name": "Root",
                                                "display_name": "Root",
                                                "node_class": "Object",
                                            }),
                                            serde_json::json!({
                                                "node_id": "i=85",
                                                "browse_name": "Objects",
                                                "display_name": "Objects",
                                                "node_class": "Object",
                                            }),
                                            serde_json::json!({
                                                "node_id": "i=86",
                                                "browse_name": "Types",
                                                "display_name": "Types",
                                                "node_class": "Object",
                                            }),
                                            serde_json::json!({
                                                "node_id": "i=87",
                                                "browse_name": "Views",
                                                "display_name": "Views",
                                                "node_class": "Object",
                                            }),
                                        ];

                                        let _ = reply.send(Ok(serde_json::json!({
                                            "success": true,
                                            "nodes": root_nodes,
                                            "browse_from": node_id,
                                            "max_depth": max_depth,
                                        })));
                                    }
                                    CommandMsg::Read { node_ids, reply } => {
                                        let mut results = Vec::new();
                                        for nid in &node_ids {
                                            let cached = cache.get_node(nid);
                                            results.push(serde_json::json!({
                                                "node_id": nid,
                                                "value": cached.as_ref().and_then(|n| n.value.clone()),
                                                "quality": cached.as_ref().and_then(|n| n.quality.clone()),
                                            }));
                                        }
                                        let _ = reply.send(Ok(serde_json::json!({
                                            "success": true,
                                            "results": results,
                                        })));
                                    }
                                    CommandMsg::Write { node_id, value, data_type, reply } => {
                                        cache.update_node_value(
                                            &node_id,
                                            value.clone(),
                                            Some("Good".to_string()),
                                            Some(chrono::Utc::now().timestamp_millis()),
                                        );
                                        let _ = reply.send(Ok(serde_json::json!({
                                            "success": true,
                                            "node_id": node_id,
                                            "written_value": value,
                                            "data_type": data_type,
                                        })));
                                    }
                                    CommandMsg::Subscribe { node_ids, interval_ms, reply } => {
                                        // Check for existing subscription with same node set
                                        let existing = cache.get_all_subscriptions();
                                        let already_subscribed = existing.iter().any(|s| {
                                            s.node_ids.len() == node_ids.len()
                                                && s.node_ids.iter().all(|n| node_ids.contains(n))
                                        });
                                        if already_subscribed {
                                            let _ = reply.send(Ok(serde_json::json!({
                                                "success": true,
                                                "message": "Subscription already exists for these nodes",
                                            })));
                                            continue;
                                        }
                                        drop(existing);

                                        let sub_id = format!("sub-{}", uuid::Uuid::new_v4());
                                        let sub = crate::types::SubscriptionInfo {
                                            subscription_id: sub_id.clone(),
                                            node_ids: node_ids.clone(),
                                            interval_ms: interval_ms.unwrap_or(1000),
                                            active: true,
                                        };
                                        cache.upsert_subscription(sub);
                                        let _ = reply.send(Ok(serde_json::json!({
                                            "success": true,
                                            "subscription_id": sub_id,
                                            "node_ids": node_ids,
                                            "interval_ms": interval_ms.unwrap_or(1000),
                                        })));
                                    }
                                    CommandMsg::Unsubscribe { node_ids, reply } => {
                                        let subs = cache.get_all_subscriptions();
                                        let mut removed = Vec::new();
                                        for sub in subs {
                                            let all_match = sub.node_ids.len() == node_ids.len()
                                                && sub.node_ids.iter().all(|n| node_ids.contains(n));
                                            if all_match {
                                                cache.remove_subscription(&sub.subscription_id);
                                                removed.push(sub.subscription_id);
                                            }
                                        }
                                        let _ = reply.send(Ok(serde_json::json!({
                                            "success": true,
                                            "removed_subscriptions": removed,
                                            "message": if removed.is_empty() {
                                                "No exact-match subscriptions found".to_string()
                                            } else {
                                                format!("Removed {} subscription(s)", removed.len())
                                            }
                                        })));
                                    }
                                    CommandMsg::ListSubscriptions { reply } => {
                                        let subs: Vec<_> = cache
                                            .get_all_subscriptions()
                                            .into_iter()
                                            .map(|s| {
                                                serde_json::json!({
                                                    "subscription_id": s.subscription_id,
                                                    "node_ids": s.node_ids,
                                                    "interval_ms": s.interval_ms,
                                                    "active": s.active,
                                                })
                                            })
                                            .collect();
                                        let _ = reply.send(Ok(serde_json::json!({
                                            "success": true,
                                            "subscriptions": subs,
                                        })));
                                    }
                                    CommandMsg::Shutdown { reply } => {
                                        eprintln!("[opcua-bridge] Shutting down background runtime");
                                        running.store(false, Ordering::SeqCst);
                                        connected.store(false, Ordering::SeqCst);
                                        cache.clear_nodes();
                                        cache.clear_subscriptions();
                                        let _ = reply.send(Ok(()));
                                    }
                                }
                            }
                            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                                if !running.load(Ordering::SeqCst) {
                                    break;
                                }
                            }
                        }
                    }

                    eprintln!("[opcua-bridge] Background runtime exited");
                });
            })
            .map_err(|e| format!("Failed to spawn OPC-UA runtime thread: {}", e))?;

        self.thread_handle = Some(handle);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Synchronous command wrappers (called from execute_command)
    //
    // These use std::sync::mpsc for reply channels because the callers
    // are async functions inside an existing tokio runtime, and
    // tokio::sync::oneshot::blocking_recv() would panic.
    // -----------------------------------------------------------------------

    /// Connect to an OPC-UA server.
    pub fn connect(&self, config: OpcUaConfig) -> Result<serde_json::Value, String> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        let tx = self.cmd_tx.as_ref().ok_or("Client not started")?;
        tx.send(CommandMsg::Connect { config, reply: reply_tx })
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.recv_timeout(std::time::Duration::from_secs(30)).map_err(|e| format!("Failed to receive response: {}", e))?
    }

    /// Disconnect from the OPC-UA server.
    pub fn disconnect(&self) -> Result<serde_json::Value, String> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        let tx = self.cmd_tx.as_ref().ok_or("Client not started")?;
        tx.send(CommandMsg::Disconnect { reply: reply_tx })
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.recv_timeout(std::time::Duration::from_secs(30)).map_err(|e| format!("Failed to receive response: {}", e))?
    }

    /// Browse the OPC-UA address space.
    pub fn browse(
        &self,
        node_id: Option<String>,
        max_depth: Option<u32>,
    ) -> Result<serde_json::Value, String> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        let tx = self.cmd_tx.as_ref().ok_or("Client not started")?;
        tx.send(CommandMsg::Browse { node_id, max_depth, reply: reply_tx })
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.recv_timeout(std::time::Duration::from_secs(30)).map_err(|e| format!("Failed to receive response: {}", e))?
    }

    /// Read values from one or more OPC-UA nodes.
    pub fn read(&self, node_ids: Vec<String>) -> Result<serde_json::Value, String> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        let tx = self.cmd_tx.as_ref().ok_or("Client not started")?;
        tx.send(CommandMsg::Read { node_ids, reply: reply_tx })
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.recv_timeout(std::time::Duration::from_secs(30)).map_err(|e| format!("Failed to receive response: {}", e))?
    }

    /// Write a value to an OPC-UA node.
    pub fn write(
        &self,
        node_id: String,
        value: serde_json::Value,
        data_type: Option<String>,
    ) -> Result<serde_json::Value, String> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        let tx = self.cmd_tx.as_ref().ok_or("Client not started")?;
        tx.send(CommandMsg::Write { node_id, value, data_type, reply: reply_tx })
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.recv_timeout(std::time::Duration::from_secs(30)).map_err(|e| format!("Failed to receive response: {}", e))?
    }

    /// Subscribe to data change notifications for one or more nodes.
    pub fn subscribe(
        &self,
        node_ids: Vec<String>,
        interval_ms: Option<u64>,
    ) -> Result<serde_json::Value, String> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        let tx = self.cmd_tx.as_ref().ok_or("Client not started")?;
        tx.send(CommandMsg::Subscribe { node_ids, interval_ms, reply: reply_tx })
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.recv_timeout(std::time::Duration::from_secs(30)).map_err(|e| format!("Failed to receive response: {}", e))?
    }

    /// Unsubscribe from data change notifications.
    pub fn unsubscribe(&self, node_ids: Vec<String>) -> Result<serde_json::Value, String> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        let tx = self.cmd_tx.as_ref().ok_or("Client not started")?;
        tx.send(CommandMsg::Unsubscribe { node_ids, reply: reply_tx })
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.recv_timeout(std::time::Duration::from_secs(30)).map_err(|e| format!("Failed to receive response: {}", e))?
    }

    /// List all active subscriptions.
    pub fn list_subscriptions(&self) -> Result<serde_json::Value, String> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        let tx = self.cmd_tx.as_ref().ok_or("Client not started")?;
        tx.send(CommandMsg::ListSubscriptions { reply: reply_tx })
            .map_err(|e| format!("Failed to send command: {}", e))?;
        reply_rx.recv_timeout(std::time::Duration::from_secs(30)).map_err(|e| format!("Failed to receive response: {}", e))?
    }

    /// Stop the background runtime gracefully.
    pub fn stop(&mut self) {
        if let Some(tx) = self.cmd_tx.take() {
            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
            let _ = tx.send(CommandMsg::Shutdown { reply: reply_tx });
            let _ = reply_rx.recv_timeout(std::time::Duration::from_secs(5));
        }
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for OpcUaClientManager {
    fn drop(&mut self) {
        self.stop();
    }
}
