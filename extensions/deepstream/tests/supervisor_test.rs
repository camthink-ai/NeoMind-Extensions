//! Integration tests for SidecarSupervisor crash recovery + backoff (Task 2.4).
//!
//! Uses the REAL wall clock because the mock sidecar's MOCK_DIE_AT_SECONDS thread
//! uses Python `time.sleep`, which is unaffected by tokio::time::pause. Each test
//! therefore asserts rough wall-clock bounds rather than pausing the runtime.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::time::timeout;

use neomind_extension_deepstream::protocol::SidecarEvent;
use neomind_extension_deepstream::sidecar::{SidecarHandle, SidecarSupervisor};

fn mock_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/mock_sidecar.py")
}

/// Helper: build a supervisor pointed at the mock sidecar with one extra env var.
fn build_supervisor(env_key: &str, env_val: &str) -> Arc<SidecarSupervisor> {
    Arc::new(
        SidecarSupervisor::new("python3", mock_script())
            .with_env(env_key, env_val),
    )
}

/// Tracks each restart's timestamp and the fresh SidecarHandle delivered by the
/// supervisor's `on_restart` callback.
struct RestartCollector {
    timestamps: Mutex<Vec<Instant>>,
    handles: Mutex<Vec<Arc<SidecarHandle>>>,
    count: AtomicUsize,
}

impl RestartCollector {
    fn new() -> Self {
        Self {
            timestamps: Mutex::new(Vec::new()),
            handles: Mutex::new(Vec::new()),
            count: AtomicUsize::new(0),
        }
    }

    fn restart_count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }

    /// Internal helper so the test closure can re-use the same Arc without
    /// moving it. Used by both tests via `cb_collector.make_callback_ref()`.
    fn make_callback_ref(self: &Arc<Self>) -> impl Fn(Arc<SidecarHandle>) + Send + Sync + 'static {
        let inner = self.clone();
        move |handle: Arc<SidecarHandle>| {
            inner.timestamps.lock().unwrap().push(Instant::now());
            inner.handles.lock().unwrap().push(handle);
            inner.count.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// supervisor_respawns_once_after_crash:
///   - mock dies at 2s
///   - first backoff is 1s
///   - on_restart fires within ~4s of start with a fresh handle
///   - the fresh handle emits a new Ready event
#[tokio::test]
async fn supervisor_respawns_once_after_crash() {
    let sup = build_supervisor("MOCK_DIE_AT_SECONDS", "2");
    let collector = Arc::new(RestartCollector::new());
    let cb_collector = collector.clone();

    let start = Instant::now();
    let (initial_handle, watch_task) = sup
        .clone()
        .start(move |h| cb_collector.make_callback_ref()(h))
        .await
        .expect("supervisor start failed");

    // 1. Receive the initial Ready from the first sidecar instance.
    let first_ready = timeout(Duration::from_secs(10), initial_handle.recv())
        .await
        .expect("initial ready timed out")
        .expect("initial stdout closed before ready");
    assert!(
        matches!(first_ready, SidecarEvent::Ready { .. }),
        "expected initial Ready, got {:?}",
        first_ready
    );

    // 2. Wait for the crash (mock dies at 2s) + 1s backoff + respawn + Python boot.
    //    Bound: crash at ~2s, backoff 1s, boot ~1s → expect restart by ~5s. Allow up to 12s
    //    for CI jitter, but assert the floor to make sure we're not seeing a spurious restart.
    let restart_deadline = Duration::from_secs(12);
    timeout(restart_deadline, async {
        while collector.restart_count() == 0 {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("on_restart did not fire within 12s of sidecar crash");

    let elapsed = start.elapsed();
    // Restart cannot have happened before the crash (2s). Backoff adds ≥1s.
    // So the earliest legitimate restart is ~3s after start; pad down to 2.5s
    // to avoid CI flake while still catching bugs that fire instantly.
    assert!(
        elapsed >= Duration::from_millis(2500),
        "restart fired too early: {:?} (expected >= 2.5s)",
        elapsed
    );

    // 3. The new handle must be usable — read a fresh Ready from it.
    let handles = collector.handles.lock().unwrap();
    assert_eq!(handles.len(), 1, "expected exactly one restart handle");
    let new_handle = handles[0].clone();
    drop(handles);

    let new_ready = timeout(Duration::from_secs(10), new_handle.recv())
        .await
        .expect("respawned ready timed out")
        .expect("respawned stdout closed before ready");
    assert!(
        matches!(new_ready, SidecarEvent::Ready { .. }),
        "expected respawned Ready, got {:?}",
        new_ready
    );

    // 4. Clean shutdown.
    drop(initial_handle);
    sup.shutdown().await.expect("shutdown failed");
    let _ = timeout(Duration::from_secs(5), watch_task).await;
}

/// supervisor_backoff_escalates_across_restarts:
///   - mock dies at 1s, so it crashes repeatedly
///   - measure timestamps of on_restart invocations
///   - assert inter-restart gaps escalate: 2nd >= 1s after 1st, 3rd >= 2s after 2nd
///   - cap at 3 restarts to keep test wall time ~8s
#[tokio::test]
async fn supervisor_backoff_escalates_across_restarts() {
    let sup = build_supervisor("MOCK_DIE_AT_SECONDS", "1");
    let collector = Arc::new(RestartCollector::new());
    let cb_collector = collector.clone();

    let (_initial_handle, watch_task) = sup
        .clone()
        .start(move |h| cb_collector.make_callback_ref()(h))
        .await
        .expect("supervisor start failed");

    // Wait for 3 restarts. Worst-case wall time:
    //   crash1 @ ~1s → backoff 1s → restart1 @ ~2s
    //   crash2 @ ~3s → backoff 2s → restart2 @ ~5s
    //   crash3 @ ~6s → backoff 5s → restart3 @ ~11s
    // Allow generous slack for CI.
    let deadline = Duration::from_secs(30);
    timeout(deadline, async {
        while collector.restart_count() < 3 {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("did not observe 3 restarts within 30s");

    let timestamps = collector.timestamps.lock().unwrap().clone();
    assert_eq!(timestamps.len(), 3, "expected exactly 3 restart timestamps");

    // Gap from restart1 → restart2 must be >= backoff[1] = 2s minus the mock's
    // 1s crash delay, i.e. >= 1s. Use 800ms as a forgiving floor.
    let gap_1_to_2 = timestamps[1].duration_since(timestamps[0]);
    assert!(
        gap_1_to_2 >= Duration::from_millis(800),
        "inter-restart gap 1→2 too short: {:?} (expected >= 800ms)",
        gap_1_to_2
    );

    // Gap from restart2 → restart3 must be >= backoff[2] = 5s minus the 1s crash delay,
    // i.e. >= ~2s of backoff. Use 1.8s as a forgiving floor.
    let gap_2_to_3 = timestamps[2].duration_since(timestamps[1]);
    assert!(
        gap_2_to_3 >= Duration::from_millis(1800),
        "inter-restart gap 2→3 too short: {:?} (expected >= 1.8s)",
        gap_2_to_3
    );

    // And it must escalate: gap_2_to_3 > gap_1_to_2.
    assert!(
        gap_2_to_3 > gap_1_to_2,
        "backoff did not escalate: gap1={:?}, gap2={:?}",
        gap_1_to_2,
        gap_2_to_3
    );

    sup.shutdown().await.expect("shutdown failed");
    let _ = timeout(Duration::from_secs(5), watch_task).await;
}
