//! Integration tests for config replay (Task 4.3 — spec §4.7).
//!
//! These tests spawn the mock sidecar, add streams directly to a
//! StreamManager (in-memory, no sidecar I/O), then call `replay_to` /
//! `replay_to_with_timeout` and verify the result.

use std::path::PathBuf;
use std::time::Duration;

use neomind_extension_deepstream::protocol::ControlMessage;
use neomind_extension_deepstream::sidecar::SidecarHandle;
use neomind_extension_deepstream::stream_manager::{
    StreamConfig, StreamManager, StreamSource, StreamStatus,
};

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

/// Convenience: minimal StreamConfig with the given id.
fn cfg(id: &str) -> StreamConfig {
    StreamConfig {
        stream_id: id.to_string(),
        source: StreamSource {
            source_type: "rtsp".to_string(),
            url: format!("rtsp://example/{id}"),
            rtsp_transport: None,
            latency_ms: None,
            retry_count: None,
        },
        model: "yolov8n".to_string(),
        model_config: None,
        tracker: None,
        analytics: None,
        output: None,
        events: None,
    }
}

#[tokio::test]
async fn replay_to_happy_path_replays_5_streams() {
    let (handle, _reader_task) = SidecarHandle::spawn("python3", &mock_script())
        .await
        .expect("spawn");
    drain_handshake(&handle).await;

    let mgr = StreamManager::new(32);
    for i in 1..=5 {
        mgr.add(cfg(&format!("s{i}"))).expect("add");
    }

    let summary = mgr.replay_to(&handle).await;

    assert_eq!(summary.succeeded.len(), 5, "all 5 should succeed");
    assert!(summary.failed.is_empty(), "no failures expected");

    // Each stream should be Running with an rtsp_url assigned.
    for state in mgr.list() {
        assert_eq!(
            state.status,
            StreamStatus::Running,
            "stream {} should be Running",
            state.config.stream_id
        );
        assert!(
            state.rtsp_url.is_some(),
            "stream {} should have rtsp_url",
            state.config.stream_id
        );
    }

    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn replay_to_records_timeout_when_no_response() {
    // Spawn mock with MOCK_IGNORE_ADD_STREAM=true — stream_added never comes.
    let (handle, _reader_task) = SidecarHandle::spawn_with_env(
        "python3",
        &mock_script(),
        [("MOCK_IGNORE_ADD_STREAM".into(), "true".into())],
        None,
    )
    .await
    .expect("spawn");
    drain_handshake(&handle).await;

    let mgr = StreamManager::new(32);
    mgr.add(cfg("t1")).expect("add t1");
    mgr.add(cfg("t2")).expect("add t2");

    // Use a short 1s per-stream timeout so the test completes in ~2s.
    let summary = mgr
        .replay_to_with_timeout(&handle, Duration::from_secs(1))
        .await;

    assert!(summary.succeeded.is_empty(), "nothing should succeed");
    assert_eq!(summary.failed.len(), 2, "both streams should fail");

    for f in &summary.failed {
        assert!(
            f.error.contains("timeout"),
            "failure error should mention timeout, got: {}",
            f.error
        );
    }

    // Each stream should have transitioned to Error.
    for state in mgr.list() {
        assert_eq!(
            state.status,
            StreamStatus::Error,
            "stream {} should be Error",
            state.config.stream_id
        );
    }

    handle.shutdown().await.unwrap();
}
