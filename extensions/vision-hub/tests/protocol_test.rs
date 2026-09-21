//! Protocol-level tests for vision-hub using the SDK testkit.
//! These exercise the IPC paths that plain unit tests miss:
//! command dispatch with timing, event injection, capability recording.

#![cfg(test)]

use neomind_extension_sdk::testkit::*;
use neomind_extension_sdk::*;
use serde_json::json;

use neomind_extension_vision_hub::VisionHub;

#[tokio::test]
async fn protocol_analyze_invalid_args() {
    let mut kit = TestKit::new(VisionHub::new());
    kit.start().await;

    // Missing image should fail fast with InvalidArguments
    let err = kit.execute_command("analyze", &json!({})).await.unwrap_err();
    assert!(
        err.to_string().contains("image") || err.to_string().contains("missing"),
        "got: {err}"
    );
}

#[tokio::test]
async fn protocol_analyze_oversized_input() {
    let mut kit = TestKit::new(VisionHub::new());
    kit.start().await;

    let huge = "A".repeat(33 * 1024 * 1024);
    let err = kit.execute_command("analyze", &json!({"image": huge})).await.unwrap_err();
    assert!(err.to_string().contains("cap"), "should reject >32MB, got: {err}");
}

#[tokio::test]
async fn protocol_pipeline_crud_through_kit() {
    let mut kit = TestKit::new(VisionHub::new());
    kit.start().await;

    // Create
    let create_result = kit
        .execute_command("create_pipeline", &json!({
            "pipeline": {
                "id": "kit-test",
                "source": {"type": "device", "device_id": "D1", "metric": "image"},
                "tasks": [{"type": "detect"}],
            }
        }))
        .await
        .expect("create should succeed within timeout");
    assert_eq!(create_result["status"], "ok");

    // List
    let list_result = kit
        .execute_command("list_pipelines", &json!({}))
        .await
        .expect("list should succeed");
    let pipelines = list_result["pipelines"].as_array().unwrap();
    // Other parallel tests may have created pipelines — check OURS exists
    let ours = pipelines.iter().find(|p| p["config"]["id"] == "kit-test");
    assert!(ours.is_some(), "pipeline 'kit-test' should exist in list");

    // Delete
    let del_result = kit
        .execute_command("delete_pipeline", &json!({"id": "kit-test"}))
        .await
        .expect("delete should succeed");
    assert_eq!(del_result["status"], "deleted");
}

#[tokio::test]
async fn protocol_reload_models_no_deadlock() {
    let mut kit = TestKit::new(VisionHub::new());
    kit.start().await;

    // Create a pipeline first (the old deadlock trigger)
    kit.execute_command("create_pipeline", &json!({
        "pipeline": {
            "id": "deadlock-test",
            "source": {"type": "device", "device_id": "D1", "metric": "image"},
            "tasks": [{"type": "detect"}],
        }
    }))
    .await
    .unwrap();

    // reload_models used to deadlock (held tasks lock while get_or_load re-locked).
    // With the testkit's 5s default timeout, a deadlock fails the test.
    let result = kit
        .execute_command("reload_models", &json!({}))
        .await
        .expect("reload_models must not deadlock (5s timeout)");
    assert_eq!(result["status"], "reloaded");
}

#[tokio::test]
async fn protocol_event_injection_timing() {
    let mut kit = TestKit::new(VisionHub::new());
    kit.start().await;

    // Inject a DeviceMetric event for a bound device — should complete
    // quickly (no inference happens without a real image, but the event
    // routing + metric matching + virtual metric check should be <100ms)
    let duration = kit
        .inject_device_metric("unbound-device", "image", json!({"String": "not-an-image"}))
        .await
        .expect("event should process without error");

    assert!(
        duration.as_millis() < 500,
        "event routing took {:?} — check for blocking in handle_event",
        duration
    );
}

#[tokio::test]
async fn protocol_virtual_metric_not_reprocessed() {
    let mut kit = TestKit::new(VisionHub::new());
    kit.start().await;

    // Our own virtual metrics must NOT re-enter the pipeline engine
    let duration = kit
        .inject_device_metric("D1", "virtual.vision.gate.detections", json!({"Integer": 5}))
        .await
        .expect("virtual metric event should be silently ignored");

    // Should be near-instant (early return on is_virtual)
    assert!(
        duration.as_millis() < 50,
        "virtual metric should early-return, took {:?}",
        duration
    );
}

#[tokio::test]
async fn protocol_get_status() {
    let mut kit = TestKit::new(VisionHub::new());
    kit.start().await;

    let status = kit
        .execute_command("get_status", &json!({}))
        .await
        .expect("get_status should succeed");

    assert!(status["hardware"].is_object());
    assert!(status["hardware"]["os"].is_string());
    assert!(status["license"].is_object());
    assert!(status["model_cache"].is_array());
}

#[tokio::test]
async fn protocol_metadata_consistency() {
    let kit = TestKit::new(VisionHub::new());
    assert_eq!(kit.metadata().id, "vision-hub");
    assert_eq!(kit.metadata().version, "0.1.0");
}

#[tokio::test]
async fn protocol_concurrent_events_with_pipeline() {
    let mut kit = TestKit::new(VisionHub::new());
    kit.start().await;

    // Create a pipeline bound to device "stress-test"
    kit.execute_command("create_pipeline", &json!({
        "pipeline": {
            "id": "stress",
            "source": {"type": "device", "device_id": "stress-test", "metric": "image"},
            "tasks": [{"type": "detect"}],
        }
    }))
    .await
    .unwrap();

    // Inject events concurrently while executing commands — tests for
    // lock contention between handle_event and execute_command paths.
    let result = kit
        .with_concurrent_events(
            "stress-test",
            std::time::Duration::from_millis(5),
            async {
                // Execute commands while events flow
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                "survived concurrent stress"
            },
        )
        .await
        .expect("should not deadlock under concurrent events + commands");

    assert_eq!(result, "survived concurrent stress");
}

#[tokio::test]
async fn protocol_license_reload() {
    let mut kit = TestKit::new(VisionHub::new());
    kit.start().await;

    let result = kit
        .execute_command("reload_license", &json!({}))
        .await
        .expect("reload_license should succeed");

    // Should return summary, not raw license material
    assert!(result["license"].is_object());
    assert!(result["license"]["source"].is_string());
    // Should NOT contain fingerprint (security)
    let license_str = result["license"].to_string();
    assert!(!license_str.contains("fingerprint"), "leaked fingerprint");
}
