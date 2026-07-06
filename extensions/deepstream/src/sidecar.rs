//! SidecarHandle — supervisor's handle to one Python sidecar process instance.
//!
//! On crash (Task 2.4), the supervisor drops the old handle and creates a new one.
//! The stdout reader task runs for the lifetime of one sidecar. It owns the BufReader
//! so leftover bytes from a multi-event chunk are preserved.
//!
//! Concurrency: stdin/stdout/child are independent locks so heartbeat writes (Task 2.3)
//! can't block user `add_stream` writes.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{mpsc, Mutex};

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
