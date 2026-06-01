//! Integration tests for homeassistant-bridge extension.
//!
//! Spawns the Python mock HA server as a subprocess for testing.
//! Requires Python 3 and the `aiohttp` package to be installed.
//! All tests are #[ignore] gated — run with:
//!
//!     cargo test -p homeassistant-bridge -- --ignored

use std::process::{Child, Command};
use std::time::Duration;

use neomind_extension_homeassistant_bridge::HomeAssistantBridgeExtension;
use neomind_extension_sdk::Extension;

// ---------------------------------------------------------------------------
// Mock server management
// ---------------------------------------------------------------------------

struct MockHaServer {
    child: Child,
    port: u16,
}

impl MockHaServer {
    fn start(port: u16) -> Self {
        let script_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/simulators/ha_mock_server.py");

        let child = Command::new("python3")
            .arg(&script_path)
            .arg("--port")
            .arg(port.to_string())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect(
                "Failed to start Python mock HA server. \
                 Ensure Python 3 and aiohttp are installed: pip install aiohttp",
            );

        // Wait for server to start
        std::thread::sleep(Duration::from_secs(2));

        Self { child, port }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for MockHaServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// Use a consistent port for all tests (sequential execution)
const TEST_PORT: u16 = 18130;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_connection() {
    let _server = MockHaServer::start(TEST_PORT);

    let ext = HomeAssistantBridgeExtension::new();

    let result = ext
        .execute_command(
            "connect",
            &serde_json::json!({
                "haUrl": format!("http://127.0.0.1:{}", TEST_PORT),
                "token": "test-token",
                "domains": "sensor,light,switch,climate"
            }),
        )
        .await
        .unwrap();

    assert_eq!(result["success"], true);

    let _ = ext
        .execute_command("disconnect", &serde_json::json!({}))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_get_states() {
    let _server = MockHaServer::start(TEST_PORT);

    let ext = HomeAssistantBridgeExtension::new();

    let _ = ext
        .execute_command(
            "connect",
            &serde_json::json!({
                "haUrl": format!("http://127.0.0.1:{}", TEST_PORT),
                "token": "test-token",
                "domains": "sensor,light,switch,climate"
            }),
        )
        .await
        .unwrap();

    // Give auto-sync time to fetch states
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Trigger metrics to force a sync
    let _ = ext.produce_metrics().unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // List entities
    let list = ext
        .execute_command("list_entities", &serde_json::json!({}))
        .await
        .unwrap();

    assert_eq!(list["success"], true);
    let entities = list["entities"].as_array().unwrap();

    // Should have 5 pre-populated entities
    assert!(
        entities.len() >= 5,
        "Expected at least 5 entities, got {}",
        entities.len()
    );

    // Verify specific entities exist
    let entity_ids: Vec<&str> = entities
        .iter()
        .filter_map(|e| e["entity_id"].as_str())
        .collect();
    assert!(entity_ids.contains(&"sensor.temperature"));
    assert!(entity_ids.contains(&"light.living_room"));
    assert!(entity_ids.contains(&"switch.kitchen"));
    assert!(entity_ids.contains(&"climate.bedroom"));

    let _ = ext
        .execute_command("disconnect", &serde_json::json!({}))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_call_service() {
    let _server = MockHaServer::start(TEST_PORT);

    let ext = HomeAssistantBridgeExtension::new();

    let _ = ext
        .execute_command(
            "connect",
            &serde_json::json!({
                "haUrl": format!("http://127.0.0.1:{}", TEST_PORT),
                "token": "test-token",
                "domains": "sensor,light,switch,climate"
            }),
        )
        .await
        .unwrap();

    // Sync entities first
    let _ = ext.produce_metrics().unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Turn on the kitchen switch
    let result = ext
        .execute_command(
            "call_service",
            &serde_json::json!({
                "domain": "switch",
                "service": "turn_on",
                "entityId": "switch.kitchen"
            }),
        )
        .await
        .unwrap();

    assert_eq!(result["success"], true);

    // Verify state changed
    let state = ext
        .execute_command(
            "get_state",
            &serde_json::json!({"entityId": "switch.kitchen"}),
        )
        .await
        .unwrap();

    assert_eq!(state["entity"]["state"], "on");

    // Turn off
    let result2 = ext
        .execute_command(
            "call_service",
            &serde_json::json!({
                "domain": "switch",
                "service": "turn_off",
                "entityId": "switch.kitchen"
            }),
        )
        .await
        .unwrap();

    assert_eq!(result2["success"], true);

    let _ = ext
        .execute_command("disconnect", &serde_json::json!({}))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_get_areas() {
    let _server = MockHaServer::start(TEST_PORT);

    let ext = HomeAssistantBridgeExtension::new();

    let _ = ext
        .execute_command(
            "connect",
            &serde_json::json!({
                "haUrl": format!("http://127.0.0.1:{}", TEST_PORT),
                "token": "test-token"
            }),
        )
        .await
        .unwrap();

    let result = ext
        .execute_command("get_areas", &serde_json::json!({}))
        .await
        .unwrap();

    assert_eq!(result["success"], true);
    let areas = result["areas"].as_array().unwrap();
    assert_eq!(areas.len(), 3);

    let area_names: Vec<&str> = areas
        .iter()
        .filter_map(|a| a["name"].as_str())
        .collect();
    assert!(area_names.contains(&"Living Room"));
    assert!(area_names.contains(&"Bedroom"));
    assert!(area_names.contains(&"Kitchen"));

    let _ = ext
        .execute_command("disconnect", &serde_json::json!({}))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_produce_metrics() {
    let _server = MockHaServer::start(TEST_PORT);

    let ext = HomeAssistantBridgeExtension::new();

    let _ = ext
        .execute_command(
            "connect",
            &serde_json::json!({
                "haUrl": format!("http://127.0.0.1:{}", TEST_PORT),
                "token": "test-token",
                "domains": "sensor"
            }),
        )
        .await
        .unwrap();

    // Trigger sync
    tokio::time::sleep(Duration::from_millis(500)).await;
    let metrics = ext.produce_metrics().unwrap();

    // Should have: ha.connection, ha.entities_count, ha.total_commands
    assert!(metrics.len() >= 3);

    let conn = metrics.iter().find(|m| m.name == "ha.connection").unwrap();
    match &conn.value {
        neomind_extension_sdk::ParamMetricValue::Integer(v) => assert_eq!(*v, 1),
        _ => panic!("Expected Integer"),
    }

    let entity_count = metrics.iter().find(|m| m.name == "ha.entities_count").unwrap();
    match &entity_count.value {
        neomind_extension_sdk::ParamMetricValue::Integer(v) => assert!(*v >= 2),
        _ => panic!("Expected Integer"),
    }

    let _ = ext
        .execute_command("disconnect", &serde_json::json!({}))
        .await
        .unwrap();
}
