//! Integration tests for SidecarHandle spawn + handshake.
//!
//! These tests spawn the mock_sidecar.py script (no GPU/DeepStream dependency)
//! and verify the Rust↔Python JSONL protocol end-to-end through real pipes.

use std::path::PathBuf;
use std::time::Duration;

use tokio::time::timeout;

use neomind_extension_deepstream::protocol::{ControlMessage, SidecarEvent};
use neomind_extension_deepstream::sidecar::SidecarHandle;

fn mock_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/mock_sidecar.py")
}

#[tokio::test]
async fn spawn_emits_ready_then_hello_receives_hello_ack() {
    let script = mock_script();
    let (handle, reader_task) = SidecarHandle::spawn("python3", &script)
        .await
        .expect("spawn failed");

    // 1. Expect `ready` within 10s (covers Python boot).
    let ready = timeout(Duration::from_secs(10), handle.recv())
        .await
        .expect("ready timed out (>10s)")
        .expect("stdout reader closed before ready");
    assert!(matches!(ready, SidecarEvent::Ready { .. }), "got {:?}", ready);

    // 2. Send hello (handshake).
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
        .expect("send hello failed");

    // 3. Expect hello_ack within 2s.
    let ack = timeout(Duration::from_secs(2), handle.recv())
        .await
        .expect("hello_ack timed out (>2s)")
        .expect("stdout reader closed before hello_ack");
    match ack {
        SidecarEvent::HelloAck { max_streams, .. } => {
            assert_eq!(max_streams, 32);
        }
        other => panic!("expected HelloAck, got {:?}", other),
    }

    // 4. Graceful shutdown.
    handle.shutdown().await.expect("shutdown failed");
    // Reader task should complete shortly after shutdown.
    let _ = timeout(Duration::from_secs(5), reader_task).await;
}

#[tokio::test]
async fn add_stream_returns_stream_added() {
    let script = mock_script();
    let (handle, reader_task) = SidecarHandle::spawn("python3", &script)
        .await
        .expect("spawn failed");

    // Drain ready.
    let _ready = handle.recv().await.expect("ready");

    // Send hello (handshake).
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

    // Add a stream.
    handle
        .send(&ControlMessage::AddStream {
            id: "r1".into(),
            config: serde_json::json!({
                "stream_id": "cam_front",
                "source": {"type": "rtsp", "url": "rtsp://example/test"}
            }),
        })
        .await
        .unwrap();

    let ev = timeout(Duration::from_secs(2), handle.recv())
        .await
        .expect("stream_added timeout")
        .expect("stream closed");
    match ev {
        SidecarEvent::StreamAdded { id, stream_id, rtsp_url } => {
            assert_eq!(id, "r1");
            assert_eq!(stream_id, "cam_front");
            assert!(rtsp_url.contains("cam_front"), "rtsp_url={}", rtsp_url);
        }
        other => panic!("expected StreamAdded, got {:?}", other),
    }

    handle.shutdown().await.unwrap();
    let _ = timeout(Duration::from_secs(5), reader_task).await;
}
