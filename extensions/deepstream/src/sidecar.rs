//! SidecarHandle — supervisor's handle to one Python sidecar process instance.
//!
//! On crash (Task 2.4), the supervisor drops the old handle and creates a new one.
//! The stdout reader task runs for the lifetime of one sidecar. It owns the BufReader
//! so leftover bytes from a multi-event chunk are preserved.
//!
//! Concurrency: stdin/stdout/child are independent locks so heartbeat writes (Task 2.3)
//! can't block user `add_stream` writes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use crate::protocol::{read_message, write_message, ControlMessage, ProtocolError, SidecarEvent};

/// Handle to a running Python sidecar process.
///
/// Wraps the child process, its stdin (for control messages), and an mpsc receiver
/// that drains events parsed from the child's stdout by a background reader task.
pub struct SidecarHandle {
    child: Mutex<Child>,
    stdin: Mutex<Option<ChildStdin>>,
    /// Mutex (not `&mut self`) so both the heartbeat task (Task 2.3) AND user-facing code
    /// can call recv() via shared `&SidecarHandle` references. The Mutex serializes actual
    /// recv calls — mpsc::UnboundedReceiver is single-consumer anyway.
    event_rx: Mutex<mpsc::UnboundedReceiver<SidecarEvent>>,
    /// Dedicated priority channel for `Pong` events (spec §4.6).
    ///
    /// Splitting pong from the main event_rx means the heartbeat task cannot be starved
    /// by a flood of Detection/Analytics events — even with thousands queued on event_rx,
    /// `recv_pong()` sees the next Pong immediately.
    pong_rx: Mutex<mpsc::UnboundedReceiver<SidecarEvent>>,
    /// Number of health_check pings sent since spawn (Observable for tests + diagnostics).
    /// Atomic because it's write-heavy (incremented each ping) and never needs `await`
    /// while held — a Mutex<u64> would force the heartbeat task to hold across the send.
    ping_count: AtomicU64,
}

impl SidecarHandle {
    /// Spawn the sidecar process.
    ///
    /// Returns the handle plus the stdout reader task's JoinHandle. The reader
    /// task lives for the lifetime of this sidecar; it terminates when stdout
    /// closes (child exited) or the receiver is dropped.
    pub async fn spawn(
        python_bin: &str,
        script_path: &Path,
    ) -> std::io::Result<(Self, tokio::task::JoinHandle<()>)> {
        Self::spawn_with_env(python_bin, script_path, std::iter::empty()).await
    }

    /// Spawn the sidecar with additional environment variables.
    ///
    /// Used by tests that need to enable mock modes (e.g. MOCK_IGNORE_HEALTHCHECK=true
    /// to verify heartbeat timeout behavior without an actual unresponsive process).
    pub async fn spawn_with_env(
        python_bin: &str,
        script_path: &Path,
        extra_env: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
    ) -> std::io::Result<(Self, tokio::task::JoinHandle<()>)> {
        let mut cmd = tokio::process::Command::new(python_bin);
        cmd.arg(script_path);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::inherit()); // logs to host stderr
        // Kill the child if the handle is dropped — critical for test isolation
        // (a panicking test must not leak a python process).
        cmd.kill_on_drop(true);
        cmd.envs(extra_env);

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");

        let (event_tx, event_rx) = mpsc::unbounded_channel::<SidecarEvent>();
        let (pong_tx, pong_rx) = mpsc::unbounded_channel::<SidecarEvent>();
        let reader_task = tokio::spawn(async move {
            stdout_reader_loop(stdout, event_tx, pong_tx).await;
        });

        Ok((
            Self {
                child: Mutex::new(child),
                stdin: Mutex::new(Some(stdin)),
                event_rx: Mutex::new(event_rx),
                pong_rx: Mutex::new(pong_rx),
                ping_count: AtomicU64::new(0),
            },
            reader_task,
        ))
    }

    /// Send a control message to the sidecar's stdin.
    pub async fn send(&self, msg: &ControlMessage) -> Result<(), ProtocolError> {
        let mut guard = self.stdin.lock().await;
        let stdin = guard.as_mut().ok_or_else(|| {
            ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "sidecar stdin already closed",
            ))
        })?;
        write_message(stdin, msg).await
    }

    /// Receive the next event from the sidecar's stdout.
    ///
    /// Returns `None` when the stdout reader task has terminated and the channel
    /// is drained (i.e. the sidecar has exited or its stdout closed).
    pub async fn recv(&self) -> Option<SidecarEvent> {
        let mut rx = self.event_rx.lock().await;
        rx.recv().await
    }

    /// Receive the next Pong event from the dedicated priority channel.
    ///
    /// Used by the heartbeat task; cannot be starved by event_rx floods (spec §4.6).
    /// Returns `None` when the stdout reader task has terminated (channel closed).
    pub async fn recv_pong(&self) -> Option<SidecarEvent> {
        let mut rx = self.pong_rx.lock().await;
        rx.recv().await
    }

    /// Number of health_check pings sent since spawn.
    /// Observable for tests + diagnostics.
    pub fn ping_count(&self) -> u64 {
        self.ping_count.load(Ordering::SeqCst)
    }

    /// Graceful shutdown.
    ///
    /// Strategy:
    ///   1. Close stdin by taking and dropping the ChildStdin — sends EOF to the sidecar.
    ///      The sidecar's main loop sees EOF and exits gracefully (mock emits `bye` first).
    ///   2. Wait up to 5s for the child to exit on its own.
    ///   3. If still alive after 5s, escalate to SIGKILL and reap.
    ///
    /// Returns `Err` only if the underlying wait/kill syscalls fail (not on
    /// timeout — timeout triggers the kill path which is itself best-effort).
    pub async fn shutdown(&self) -> std::io::Result<()> {
        // 1. Close stdin by taking and dropping the ChildStdin — sends EOF to the sidecar.
        //    The sidecar's main loop sees EOF and exits gracefully (mock emits `bye` first).
        {
            let mut guard = self.stdin.lock().await;
            let _taken = guard.take();  // Drops the ChildStdin, closing the pipe
        }

        // 2. Wait up to 5s for the child to exit on its own.
        let mut child = self.child.lock().await;
        match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
            Ok(Ok(_status)) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_elapsed) => {
                // 3. Graceful shutdown failed — escalate to SIGKILL.
                child.kill().await?;
                child.wait().await?;
                Ok(())
            }
        }
    }
}

/// Background task: read JSONL messages from the sidecar's stdout until EOF or error,
/// demuxing each parsed event to either the event channel or the priority pong channel.
///
/// Pong events go to `pong_tx` (consumed by the heartbeat task); everything else
/// goes to `event_tx` (consumed by user-facing recv()). This split means a flood of
/// Detection events cannot starve the heartbeat's pong wait (spec §4.6).
///
/// The BufReader is owned here so leftover bytes after a newline are preserved
/// across reads (the bug that prompted the Part A refactor).
async fn stdout_reader_loop(
    stdout: ChildStdout,
    event_tx: mpsc::UnboundedSender<SidecarEvent>,
    pong_tx: mpsc::UnboundedSender<SidecarEvent>,
) {
    let mut reader = tokio::io::BufReader::new(stdout);
    loop {
        match read_message::<_, SidecarEvent>(&mut reader).await {
            Ok(ev) => {
                // Demux: Pong goes to the dedicated priority channel,
                // everything else to event_rx. This means heartbeat pong
                // waits cannot be starved by event floods (spec §4.6).
                let is_pong = matches!(ev, SidecarEvent::Pong { .. });
                let tx = if is_pong { &pong_tx } else { &event_tx };
                if tx.send(ev).is_err() {
                    // Receiver dropped — SidecarHandle is gone. Stop reading.
                    break;
                }
            }
            Err(e) => {
                eprintln!("[deepstream] sidecar stdout reader error: {:?}", e);
                break;
            }
        }
    }
}

/// Run the heartbeat protocol against the sidecar.
///
/// Every 10s, sends a `health_check` ping. Waits up to 5s for a `pong` on the
/// dedicated priority channel. If `pong` doesn't arrive (or the channel closes),
/// fires `on_timeout` once and exits.
///
/// Cancel by aborting the spawned task (`JoinHandle::abort()`).
///
/// Per spec §4.6, pongs arrive on a dedicated channel so they cannot be
/// starved by detection/analytics event floods.
pub async fn heartbeat_loop<F>(handle: std::sync::Arc<SidecarHandle>, on_timeout: F)
where
    F: FnOnce() + Send + 'static,
{
    loop {
        // Wait 10s before next ping.
        tokio::time::sleep(Duration::from_secs(10)).await;

        // Send health_check.
        let ts = chrono::Utc::now().timestamp_millis();
        let send_result = handle.send(&ControlMessage::HealthCheck { ts }).await;
        if send_result.is_err() {
            // stdin broken — sidecar is gone.
            on_timeout();
            return;
        }
        // Increment AFTER successful send but BEFORE pong wait, so tests can
        // observe "exactly 1 ping sent" even when pong never arrives.
        handle.ping_count.fetch_add(1, Ordering::SeqCst);

        // Wait up to 5s for pong on the dedicated priority channel.
        match tokio::time::timeout(Duration::from_secs(5), handle.recv_pong()).await {
            Ok(Some(_pong)) => continue, // healthy — loop to next ping
            Ok(None) => {
                // pong channel closed — reader task ended (sidecar stdout closed).
                on_timeout();
                return;
            }
            Err(_elapsed) => {
                // No pong within 5s — sidecar unresponsive.
                on_timeout();
                return;
            }
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// SidecarSupervisor — crash recovery with exponential backoff (Task 2.4)
//
// Owns the live SidecarHandle and runs a background watch loop that respawns
// the sidecar when its stdout reader task terminates unexpectedly. Respawn is
// rate-limited by a sliding-window counter (5 restarts / 60s) and the inter-
// restart gap is governed by the BACKOFF_SCHEDULE_SECS table (spec §4.7).
//
// Exit-code classification (spec §4.7 — code 2 = DS missing = no restart,
// code 3 = GPU OOM = backoff, etc.) is intentionally NOT handled here yet;
// every unexpected reader_task termination triggers the backoff path. That
// classification is a later task.
//
// The "Restart replay protocol" (spec §4.7 — replaying stored stream configs
// after a respawn) is also out of scope. `on_restart(new_handle)` is the seam
// the future replay logic will be wired into from DeepStreamExtension.
// ───────────────────────────────────────────────────────────────────────────

/// Backoff schedule for sidecar restarts (spec §4.7).
/// Indexed by `min(restart_count_in_window, len-1)` so it caps at 30s.
const BACKOFF_SCHEDULE_SECS: &[u64] = &[1, 2, 5, 10, 30];

/// Max restarts allowed within the sliding window before marking the
/// supervisor `Failed`.
const MAX_RESTARTS_IN_WINDOW: usize = 5;

/// Sliding window length for the restart-rate limit.
const RESTART_WINDOW_SECS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorState {
    /// Watch loop is active; a sidecar is running (or about to be respawn).
    Running,
    /// `shutdown()` was called — do NOT respawn on the next reader_task exit.
    Stopping,
    /// Rate-limit tripped (5-in-60s) or spawn failed; supervisor has given up.
    Failed,
}

pub struct SidecarSupervisor {
    python_bin: String,
    script_path: PathBuf,
    extra_env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    /// Current sidecar instance + its stdout reader task JoinHandle.
    /// `None` when between restarts or after shutdown.
    current: Mutex<Option<SupervisorEntry>>,
    /// Cumulative count of restarts since `start()` (for diagnostics / metrics).
    restart_count: AtomicU64,
    /// Wall-clock timestamps of recent restarts, used for the sliding-window
    /// rate limit. Pruned to entries within `RESTART_WINDOW_SECS`.
    restart_history: Mutex<Vec<Instant>>,
    /// Supervisor state — prevents respawn after shutdown/failure.
    state: Mutex<SupervisorState>,
}

struct SupervisorEntry {
    handle: Arc<SidecarHandle>,
    reader_task: JoinHandle<()>,
}

impl SidecarSupervisor {
    pub fn new(python_bin: &str, script_path: PathBuf) -> Self {
        Self {
            python_bin: python_bin.to_string(),
            script_path,
            extra_env: Vec::new(),
            current: Mutex::new(None),
            restart_count: AtomicU64::new(0),
            restart_history: Mutex::new(Vec::new()),
            state: Mutex::new(SupervisorState::Stopping),
        }
    }

    /// Add an env var to be passed to every (re)spawn of the sidecar.
    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.extra_env.push((
            std::ffi::OsString::from(key),
            std::ffi::OsString::from(value),
        ));
        self
    }

    /// Start the supervisor: spawn the initial sidecar and launch the watch
    /// loop. Returns the initial handle (for sending user commands) and the
    /// watch-loop `JoinHandle`.
    ///
    /// The caller should hold the `JoinHandle` (dropping it does NOT cancel
    /// the loop — call `shutdown()` to stop). The `on_restart` callback is
    /// invoked on each successful respawn with the new `SidecarHandle`.
    pub async fn start<F>(
        self: Arc<Self>,
        on_restart: F,
    ) -> std::io::Result<(Arc<SidecarHandle>, JoinHandle<()>)>
    where
        F: Fn(Arc<SidecarHandle>) + Send + Sync + 'static,
    {
        // 1. Initial spawn — failure here is fatal and bubbles to the caller.
        let (handle, reader_task) = SidecarHandle::spawn_with_env(
            &self.python_bin,
            &self.script_path,
            self.extra_env.iter().cloned(),
        )
        .await?;
        let handle = Arc::new(handle);
        // Set state=Running BEFORE publishing the handle in `current` so the
        // state transition is complete before any reader_task-exit observation
        // can race a `state()` reader (I1 ordering invariant).
        *self.state.lock().await = SupervisorState::Running;
        *self.current.lock().await = Some(SupervisorEntry {
            handle: handle.clone(),
            reader_task,
        });

        // 2. Launch the watch loop. The callback is wrapped in Arc<F> so the
        //    spawned task can own it (Fn is ?Sized).
        let on_restart = Arc::new(on_restart);
        let watch_task = tokio::spawn(watch_loop(self.clone(), on_restart));

        Ok((handle, watch_task))
    }

    /// Cumulative restart count (for metrics / diagnostics).
    pub fn restart_count(&self) -> u64 {
        self.restart_count.load(Ordering::SeqCst)
    }

    /// Current supervisor state. Useful for the host to detect `Failed`
    /// (rate-limit tripped or spawn failure) and surface it to the user.
    pub async fn state(&self) -> SupervisorState {
        *self.state.lock().await
    }

    /// Initiate graceful shutdown: stop the watch loop (prevents respawn) and
    /// shut down the live sidecar.
    pub async fn shutdown(&self) -> std::io::Result<()> {
        // State guard: even if the watch loop wakes up after we tear down the
        // child, it will see Stopping and exit without respawning.
        *self.state.lock().await = SupervisorState::Stopping;

        // Take the current entry so the watch loop's next await on
        // reader_task resolves immediately, and so we can call shutdown on
        // the handle exactly once.
        let entry = self.current.lock().await.take();
        if let Some(e) = entry {
            e.handle.shutdown().await?;
        }
        Ok(())
    }
}

/// Background watch loop: waits for the current sidecar's reader task to
/// finish (child exited / stdout closed), then respawns with backoff.
///
/// Terminates without respawning when:
///   - supervisor state is not `Running` (shutdown or already-failed)
///   - sliding-window rate limit hit (5 restarts / 60s) → marks state Failed
///   - respawn spawn call fails → marks state Failed
async fn watch_loop<F>(sup: Arc<SidecarSupervisor>, on_restart: Arc<F>)
where
    F: Fn(Arc<SidecarHandle>) + Send + Sync + 'static,
{
    loop {
        // 1. Wait for the current reader_task to finish. We can't clone a
        //    JoinHandle, so take the whole entry out of the mutex (under the
        //    lock), await the reader task outside the lock, then either
        //    respawn or re-store the entry on shutdown.
        let SupervisorEntry { handle, reader_task } = {
            let mut cur = sup.current.lock().await;
            match cur.take() {
                Some(e) => e,
                None => return, // No current sidecar — supervisor shut down before we started.
            }
        };
        let _ = reader_task.await;

        // 2. Honor supervisor state — don't respawn if we're shutting down or
        //    failed. Note: shutdown() sets state=Stopping and then takes the
        //    entry from `current`. Because we hold the entry here (we already
        //    took it), shutdown() found None and couldn't shut the child down
        //    itself — so we do it here.
        {
            let st = sup.state.lock().await;
            if *st != SupervisorState::Running {
                let _ = handle.shutdown().await;
                return;
            }
        }

        // 3. Sliding-window rate limit + backoff computation.
        //    Single critical section on restart_history: prune stale entries,
        //    check the rate limit (returning the histogram length for backoff
        //    indexing if we're still within budget), then drop the guard BEFORE
        //    touching `state` to avoid any nested-lock risk (C1).
        let now = Instant::now();
        let backoff_secs = {
            let mut hist = sup.restart_history.lock().await;
            hist.retain(|t| now.duration_since(*t) < Duration::from_secs(RESTART_WINDOW_SECS));
            if hist.len() >= MAX_RESTARTS_IN_WINDOW {
                // Drop the guard explicitly before acquiring `state` (C1).
                drop(hist);
                *sup.state.lock().await = SupervisorState::Failed;
                eprintln!(
                    "[deepstream] supervisor giving up: rate limit ({} restarts in {}s window) exceeded",
                    MAX_RESTARTS_IN_WINDOW,
                    RESTART_WINDOW_SECS
                );
                return;
            }
            // Compute backoff from the current restart count in the window
            // (before recording this attempt). hist.len() is 0 on the first
            // crash → backoff[0] = 1s; the next crash → backoff[1] = 2s; etc.
            let backoff_idx = std::cmp::min(hist.len(), BACKOFF_SCHEDULE_SECS.len() - 1);
            BACKOFF_SCHEDULE_SECS[backoff_idx]
        };

        eprintln!(
            "[deepstream] sidecar crashed; backoff {}s before restart (attempt {})",
            backoff_secs,
            sup.restart_count.load(Ordering::SeqCst) + 1
        );

        // 4. Sleep the backoff. We re-check state after the sleep so a
        //    concurrent shutdown() interrupts the respawn path.
        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        {
            let st = sup.state.lock().await;
            if *st != SupervisorState::Running {
                return;
            }
        }

        // 5. Respawn. On spawn failure, mark the supervisor Failed and exit —
        //    we treat a spawn failure as fatal (distinct from a child crash).
        let (handle, reader_task) = match SidecarHandle::spawn_with_env(
            &sup.python_bin,
            &sup.script_path,
            sup.extra_env.iter().cloned(),
        )
        .await
        {
            Ok(t) => t,
            Err(e) => {
                eprintln!(
                    "[deepstream] respawn failed: {:?}; supervisor exiting",
                    e
                );
                *sup.state.lock().await = SupervisorState::Failed;
                return;
            }
        };
        let handle = Arc::new(handle);

        // Record this restart in the sliding window AFTER the successful
        // respawn (C2). The timestamp reflects when the restart actually
        // happened — not when we decided to retry — so a 30s backoff followed
        // by a spawn failure doesn't count as "a restart in the window".
        // Capture a fresh Instant because the sleep above invalidated `now`.
        sup.restart_history.lock().await.push(Instant::now());

        *sup.current.lock().await = Some(SupervisorEntry {
            handle: handle.clone(),
            reader_task,
        });
        sup.restart_count.fetch_add(1, Ordering::SeqCst);

        // 6. Notify the caller that a fresh handle is available. The future
        //    replay logic (spec §4.7) will live in this callback body.
        on_restart(handle);

        // Loop: wait for the new reader_task to exit.
    }
}
