//! Integration tests for the heartbeat task.
//!
//! Uses tokio's mock clock (start_paused) so tests run in milliseconds,
//! not the 15+ seconds of real time the heartbeat would otherwise take.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use neomind_extension_deepstream::protocol::ControlMessage;
use neomind_extension_deepstream::sidecar::{heartbeat_loop, SidecarHandle};
use std::path::PathBuf;

fn mock_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/mock_sidecar.py")
}

async fn drain_handshake(handle: &SidecarHandle) {
    // Drain ready
    let _ready = handle.recv().await.expect("ready");
    // Send hello
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

#[tokio::test(start_paused = true)]
async fn heartbeat_sends_ping_and_recovers_pong() {
    let (handle, _reader_task) = SidecarHandle::spawn("python3", &mock_script())
        .await
        .expect("spawn");
    drain_handshake(&handle).await;
    let handle = Arc::new(handle);

    let timeout_count = Arc::new(AtomicU32::new(0));
    let tc = timeout_count.clone();

    let h = handle.clone();
    let task = tokio::spawn(async move {
        heartbeat_loop(h, move || {
            tc.fetch_add(1, Ordering::SeqCst);
        })
        .await;
    });

    // Advance 10s — heartbeat should send exactly 1 ping.
    // First yield so the spawned task gets polled once and registers its sleep()
    // in the timer wheel. Then advance in small steps with yields between, so
    // the runtime can poll the spawned task (and the child's pipe I/O) between
    // clock advances.
    tokio::task::yield_now().await;
    for _ in 0..40 {
        tokio::time::advance(Duration::from_millis(250)).await;
        tokio::task::yield_now().await;
        if handle.ping_count() >= 1 {
            break;
        }
    }
    // Extra yields to let the mock respond with pong + recv_pong settle.
    for _ in 0..20 {
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(10)).await;
    }

    assert_eq!(handle.ping_count(), 1, "expected exactly 1 ping after 10s");
    assert_eq!(timeout_count.load(Ordering::SeqCst), 0, "no timeout expected");

    // Cancel cleanly
    task.abort();
    handle.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn heartbeat_fires_on_timeout_when_pong_missing() {
    // Spawn mock with MOCK_IGNORE_HEALTHCHECK=true — pong never comes back
    let (handle, _reader_task) = SidecarHandle::spawn_with_env(
        "python3",
        &mock_script(),
        [("MOCK_IGNORE_HEALTHCHECK".into(), "true".into())],
        None,
    )
    .await
    .expect("spawn");
    drain_handshake(&handle).await;
    let handle = Arc::new(handle);

    let timeout_count = Arc::new(AtomicU32::new(0));
    let tc = timeout_count.clone();

    let h = handle.clone();
    let task = tokio::spawn(async move {
        heartbeat_loop(h, move || {
            tc.fetch_add(1, Ordering::SeqCst);
        })
        .await;
    });

    // Advance 10s — 1 ping sent, mock ignores
    tokio::task::yield_now().await;
    for _ in 0..40 {
        tokio::time::advance(Duration::from_millis(250)).await;
        tokio::task::yield_now().await;
        if handle.ping_count() >= 1 {
            break;
        }
    }

    assert_eq!(handle.ping_count(), 1, "1 ping after 10s");
    assert_eq!(
        timeout_count.load(Ordering::SeqCst),
        0,
        "no timeout yet (pong window still open)"
    );

    // Advance 5s more — pong window elapses, on_timeout fires
    for _ in 0..40 {
        tokio::time::advance(Duration::from_millis(125)).await;
        tokio::task::yield_now().await;
        if timeout_count.load(Ordering::SeqCst) >= 1 {
            break;
        }
    }
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }

    assert_eq!(
        timeout_count.load(Ordering::SeqCst),
        1,
        "on_timeout should fire exactly once"
    );
    assert_eq!(
        handle.ping_count(),
        1,
        "no additional pings after timeout (loop exits)"
    );

    // Task should have exited on its own
    let join_result = tokio::time::timeout(Duration::from_secs(1), task).await;
    assert!(
        join_result.is_ok(),
        "heartbeat_loop should have exited after on_timeout"
    );

    handle.shutdown().await.unwrap();
}
