//! Integration test for the remote transport mode (路 C).
//!
//! Spawns the `sidecar_bridge.py` daemon (which in turn spawns the
//! `mock_sidecar.py` because we set `DEEPSTREAM_SIDECAR_PATH` to it) and
//! connects the Rust side via `SidecarHandle::connect_remote`. Verifies the
//! JSONL protocol is delivered byte-for-byte over TCP, the same as local
//! spawn mode. Also exercises the `SidecarSupervisor::new_remote` path.
//!
//! Stdlib-only — runs on any host (no GPU / DeepStream required). The bridge
//! script itself only depends on Python 3 stdlib.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::process::Child;

use tokio::process::Command;
use tokio::time::timeout;

use neomind_extension_deepstream::protocol::{ControlMessage, SidecarEvent};
use neomind_extension_deepstream::sidecar::{SidecarHandle, SidecarSupervisor};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn bridge_script() -> PathBuf {
    crate_root().join("sidecar").join("sidecar_bridge.py")
}

fn mock_script() -> PathBuf {
    crate_root().join("tests").join("mock_sidecar.py")
}

/// Pick a free TCP port by binding to :0 and immediately closing.
/// Avoids port collisions when tests run in parallel.
fn pick_free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind failed")
        .local_addr()
        .expect("local_addr")
        .port()
}

/// Spawn the bridge daemon pointed at the mock_sidecar.py. The mock lacks
/// DeepStream but emits the right JSONL frames, which is all we need to
/// verify the wire path.
fn spawn_bridge(port: u16) -> Child {
    let mock = mock_script();
    Command::new("python3")
        .arg(bridge_script())
        .arg("--port")
        .arg(port.to_string())
        .arg("--log-level")
        .arg("warning")
        .env("DEEPSTREAM_SIDECAR_PATH", &mock)
        .stdin(Stdio::null())
        // Inherit stderr — when the test fails the bridge's logs are visible
        // in the test runner output, which is invaluable for debugging.
        .stdout(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("failed to spawn sidecar_bridge.py")
}

/// Retry-connect until the bridge is ready. The bridge's Python boot takes
/// ~50ms on a dev box but can be slower on CI; give it a 10s window.
async fn connect_with_retry(host: &str, port: u16) -> std::io::Result<(SidecarHandle, tokio::task::JoinHandle<()>)> {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match SidecarHandle::connect_remote(host, port, None).await {
            Ok(t) => return Ok(t),
            Err(e) => {
                if std::time::Instant::now() > deadline {
                    return Err(e);
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

#[tokio::test]
async fn connect_remote_receives_ready_and_hello_ack() {
    // Mock the bridge's mock-sidecar at the protocol level: since the bridge
    // launches whatever DEEPSTREAM_SIDECAR_PATH points at, point it at our
    // existing mock_sidecar.py — same as the local-mode tests.
    let port = pick_free_port();
    let _bridge = spawn_bridge(port);

    // Connect from the Rust side (this is the exact code path the extension
    // uses when sidecar_mode=remote).
    let (handle, reader_task) = connect_with_retry("127.0.0.1", port)
        .await
        .expect("connect_remote failed");

    // 1. Bridge spawns mock_sidecar.py on connect; mock emits `ready`.
    let ready = timeout(Duration::from_secs(10), handle.recv())
        .await
        .expect("ready timed out (>10s)")
        .expect("socket closed before ready");
    assert!(matches!(ready, SidecarEvent::Ready { .. }), "got {:?}", ready);

    // 2. Send Hello — same frame as local mode.
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
        .expect("send Hello over TCP failed");

    // 3. Expect HelloAck back through the bridge.
    let ack = timeout(Duration::from_secs(5), handle.recv())
        .await
        .expect("HelloAck timed out (>5s)")
        .expect("socket closed before HelloAck");
    match ack {
        SidecarEvent::HelloAck { max_streams, .. } => assert_eq!(max_streams, 32),
        other => panic!("expected HelloAck, got {:?}", other),
    }

    // 4. Graceful shutdown via the TCP path (drop write half + bye).
    handle.shutdown().await.expect("remote shutdown failed");
    let _ = timeout(Duration::from_secs(5), reader_task).await;
}

#[tokio::test]
async fn remote_supervisor_starts_and_shuts_down_cleanly() {
    // Cover the `SidecarSupervisor::new_remote` constructor — this is the
    // entry point `build_supervisor()` calls when sidecar_mode='remote'.
    let port = pick_free_port();
    let _bridge = spawn_bridge(port);

    // First connect (will be retried until the bridge is up). We don't keep
    // this — we just probe. The bridge will tear down its spawned mock when
    // we drop; we then wait briefly for the bridge's per-client lock to
    // release before starting the supervisor.
    //
    // NOTE: the bridge rejects a second concurrent client with `bridge_busy`,
    // so we can't overlap this probe with the supervisor's connect.
    {
        let _probe = connect_with_retry("127.0.0.1", port)
            .await
            .expect("probe connect failed");
        // Drop probe → socket closes → bridge SIGTERMs the mock sidecar and
        // releases the per-client lock.
    }
    // Small grace period for the bridge's SIGTERM + lock release.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let sup = Arc::new(SidecarSupervisor::new_remote("127.0.0.1", port));
    assert!(sup.is_remote(), "supervisor should report remote mode");

    let (handle, watch_task) = sup
        .clone()
        .start(|_handle| {
            // No-op — we don't trigger a restart in this test.
        })
        .await
        .expect("supervisor start failed");

    // Drain ready — proves the bridge spawned the sidecar via the watch path.
    let ready = timeout(Duration::from_secs(10), handle.recv())
        .await
        .expect("ready timed out")
        .expect("socket closed before ready");
    assert!(matches!(ready, SidecarEvent::Ready { .. }));

    // Clean supervisor shutdown — no zombie bridge-spawned sidecar.
    sup.shutdown().await.expect("supervisor shutdown failed");
    let _ = timeout(Duration::from_secs(5), watch_task).await;
}
