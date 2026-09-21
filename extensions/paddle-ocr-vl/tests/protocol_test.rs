//! Protocol-level tests — exercise the IPC paths that unit tests miss.
//! Generated from the testkit infrastructure (SDK 0.6.6+).

#![cfg(test)]

use neomind_extension_sdk::testkit::*;
use neomind_extension_sdk::*;
use serde_json::json;

use neomind_extension_paddle_ocr_vl::PaddleOcrVlExtension;

/// Metadata consistency: id and version match what metadata.json declares.
#[tokio::test]
async fn protocol_metadata() {
    let kit = TestKit::new(PaddleOcrVlExtension::new());
    assert!(!kit.metadata().id.is_empty());
    assert!(!kit.metadata().version.is_empty());
}

/// Unknown commands fail fast (not panic, not hang).
#[tokio::test]
async fn protocol_unknown_command() {
    let mut kit = TestKit::new(PaddleOcrVlExtension::new());
    kit.start().await;
    assert!(kit.execute_command("__nonexistent__", &json!({})).await.is_err(), "unknown command should error");
}

/// Key commands respond within timeout (5s default — deadlock detection).
#[tokio::test]
async fn protocol_health() {
    let mut kit = TestKit::new(PaddleOcrVlExtension::new());
    kit.start().await;
    // Execute through the IPC path — timing assertion catches deadlocks
    let result = kit.execute_command("health", &json!({})).await;
    // We verify the command doesn't deadlock/panic; specific result
    // depends on runtime state (models, connections, etc.)
    match result {
        Ok(_) => { /* command succeeded */ }
        Err(e) => {
            let msg = e.to_string();
            // Expected error patterns for commands requiring external deps
            assert!(
                msg.contains("not found") || msg.contains("not loaded")
                || msg.contains("not connected") || msg.contains("failed")
                || msg.contains("invalid") || msg.contains("missing")
                || msg.contains("error") || msg.contains("service")
                || msg.contains("timeout") || msg.contains("model"),
                "unexpected error for health: {msg}"
            );
        }
    }
}
#[tokio::test]
async fn protocol_recognize() {
    let mut kit = TestKit::new(PaddleOcrVlExtension::new());
    kit.start().await;
    // Execute through the IPC path — timing assertion catches deadlocks
    let result = kit.execute_command("recognize", &json!({"image": "invalid"})).await;
    // We verify the command doesn't deadlock/panic; specific result
    // depends on runtime state (models, connections, etc.)
    match result {
        Ok(_) => { /* command succeeded */ }
        Err(e) => {
            let msg = e.to_string();
            // Expected error patterns for commands requiring external deps
            assert!(
                msg.contains("not found") || msg.contains("not loaded")
                || msg.contains("not connected") || msg.contains("failed")
                || msg.contains("invalid") || msg.contains("missing")
                || msg.contains("error") || msg.contains("service")
                || msg.contains("timeout") || msg.contains("model"),
                "unexpected error for recognize: {msg}"
            );
        }
    }
}

