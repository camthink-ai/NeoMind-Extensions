//! Integration tests for Task 4.4 — execute_command handlers.
//!
//! These spawn the mock Python sidecar, build a DeepStreamExtension via the
//! `for_test` seam (skipping real init / pre-flight), and drive the command
//! surface end-to-end through the public Extension trait.

use std::path::PathBuf;
use std::sync::Arc;

use neomind_extension_deepstream::protocol::ControlMessage;
use neomind_extension_deepstream::sidecar::SidecarHandle;
use neomind_extension_deepstream::stream_manager::StreamManager;
use neomind_extension_deepstream::DeepStreamExtension;
use neomind_extension_sdk::Extension;

fn mock_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/mock_sidecar.py")
}

/// Drain the sidecar handshake: Ready → send Hello → HelloAck.
async fn drain_handshake(handle: &SidecarHandle) {
    let _ready = handle.recv().await.expect("ready");
    handle
        .send(&ControlMessage::Hello {
            rtsp_port: 8554,
            snapshot_port: 8555,
            log_level: "info".into(),
            models_dir: "/tmp".into(),
            max_streams: 32,
            snapshot_bind_addr: "0.0.0.0".into(),
        })
        .await
        .unwrap();
    let _ack = handle.recv().await.expect("hello_ack");
}

const MAX_STREAMS: u32 = 32;

async fn spawn_extension() -> (DeepStreamExtension, Arc<SidecarHandle>) {
    let (handle, _reader_task) = SidecarHandle::spawn("python3", &mock_script())
        .await
        .expect("spawn");
    drain_handshake(&handle).await;
    let handle_arc = Arc::new(handle);
    let streams = Arc::new(StreamManager::new(MAX_STREAMS));
    let ext = DeepStreamExtension::for_test(streams, handle_arc.clone());
    (ext, handle_arc)
}

#[tokio::test]
async fn add_stream_command_returns_rtsp_url_from_mock() {
    let (ext, handle) = spawn_extension().await;

    let result = ext
        .execute_command(
            "add_stream",
            &serde_json::json!({
                "stream_id": "cam1",
                "config": {
                    "stream_id": "cam1",
                    "source": {"type": "rtsp", "url": "rtsp://x"},
                    "model": "yolov8n-coco"
                }
            }),
        )
        .await
        .expect("add_stream ok");

    let stream_id = result
        .get("stream_id")
        .and_then(|v| v.as_str())
        .expect("stream_id in response");
    let rtsp_url = result
        .get("rtsp_url")
        .and_then(|v| v.as_str())
        .expect("rtsp_url in response");
    assert_eq!(stream_id, "cam1");
    assert!(
        rtsp_url.contains("cam1"),
        "rtsp_url should include stream id, got {rtsp_url}"
    );

    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn list_streams_after_add_returns_1_entry() {
    let (ext, handle) = spawn_extension().await;

    ext.execute_command(
        "add_stream",
        &serde_json::json!({
            "stream_id": "cam1",
            "config": {
                "stream_id": "cam1",
                "source": {"type": "rtsp", "url": "rtsp://x"},
                "model": "yolov8n-coco"
            }
        }),
    )
    .await
    .expect("add_stream ok");

    let list = ext
        .execute_command("list_streams", &serde_json::json!({}))
        .await
        .expect("list_streams ok");
    let arr = list
        .get("streams")
        .and_then(|v| v.as_array())
        .expect("streams array");
    assert_eq!(arr.len(), 1, "expected 1 entry, got {arr:?}");
    let entry = &arr[0];
    assert_eq!(
        entry.get("stream_id").and_then(|v| v.as_str()),
        Some("cam1")
    );
    assert_eq!(
        entry.get("status").and_then(|v| v.as_str()),
        Some("running"),
        "status should be running after add succeeds"
    );

    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn remove_stream_command_clears_state() {
    let (ext, handle) = spawn_extension().await;

    ext.execute_command(
        "add_stream",
        &serde_json::json!({
            "stream_id": "cam1",
            "config": {
                "stream_id": "cam1",
                "source": {"type": "rtsp", "url": "rtsp://x"},
                "model": "yolov8n-coco"
            }
        }),
    )
    .await
    .expect("add_stream ok");

    let removed = ext
        .execute_command(
            "remove_stream",
            &serde_json::json!({"stream_id": "cam1"}),
        )
        .await
        .expect("remove_stream ok");
    assert_eq!(
        removed.get("removed").and_then(|v| v.as_str()),
        Some("cam1")
    );

    // list_streams should now be empty.
    let list = ext
        .execute_command("list_streams", &serde_json::json!({}))
        .await
        .expect("list_streams ok");
    let arr = list
        .get("streams")
        .and_then(|v| v.as_array())
        .expect("streams array");
    assert!(arr.is_empty(), "after remove, list should be empty");

    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn get_stream_info_on_missing_returns_not_found() {
    let (ext, handle) = spawn_extension().await;

    let err = ext
        .execute_command(
            "get_stream_info",
            &serde_json::json!({"stream_id": "ghost"}),
        )
        .await
        .expect_err("missing stream should error");
    match err {
        neomind_extension_sdk::ExtensionError::NotFound(msg) => {
            assert!(
                msg.contains("ghost"),
                "NotFound message should mention the id, got {msg}"
            );
        }
        other => panic!("expected NotFound, got {other:?}"),
    }

    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn diagnose_runs_checks_and_returns_status() {
    // diagnose doesn't need the sidecar — run against a fresh extension.
    let ext = DeepStreamExtension::new();

    let result = ext
        .execute_command("diagnose", &serde_json::json!({}))
        .await
        .expect("diagnose ok");

    // deepstream_installed is always present; its value depends on the host
    // (true on Jetson, false on macOS dev). Don't assert the value.
    assert!(
        result.get("deepstream_installed").is_some(),
        "deepstream_installed field missing in {result}"
    );
    assert!(
        result.get("gst_plugins_ok").is_some(),
        "gst_plugins_ok field missing in {result}"
    );
    assert!(
        result.get("pyds_available").is_some(),
        "pyds_available field missing in {result}"
    );
    assert!(
        result.get("last_check_at").is_some(),
        "last_check_at field missing in {result}"
    );
}
