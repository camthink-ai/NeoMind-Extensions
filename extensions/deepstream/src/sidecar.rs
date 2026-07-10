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
use std::sync::{Arc, OnceLock};

/// Persistent tokio runtime for long-lived sidecar I/O tasks.
///
/// The SDK's FFI bridge runs each command on an EPHEMERAL runtime that is
/// dropped when the command returns. Any `tokio::spawn`'d task (reader_loop,
/// watch_loop) and any registered I/O resource (TcpStream, ChildStdin) tied
/// to that runtime dies with it — which is why `send()` from a later command
/// fails with "A Tokio 1.x context was found, but it is being shutdown" and
/// the reader_loop silently stops draining the socket.
///
/// This static runtime outlives any single FFI call. Sidecar spawning,
/// reader_loop / watch_loop, and `send`/`recv`/`shutdown` all enter this
/// runtime's context so I/O resources and tasks live on the same reactor
/// across calls. Mirrors the voice-assistant pattern.
pub fn persistent_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to build deepstream persistent runtime")
    })
}

/// Capture the currently-entered runtime if there is one (test path); fall
/// back to the persistent runtime otherwise. Stored on `SidecarHandle` so
/// `send()` can poll the writer on the same reactor that owns it.
fn current_or_persistent_handle() -> tokio::runtime::Handle {
    match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => persistent_runtime().handle().clone(),
    }
}
use std::time::{Duration, Instant};

/// Debug logger that writes to /tmp/deepstream_debug.log (stderr is swallowed by FFI).
pub fn dbg_log(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/deepstream_debug.log")
    {
        let _ = writeln!(f, "{} {msg}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0));
    }
}

use tokio::io::AsyncRead;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::process::{Child, ChildStdin};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use crate::event_router::EventRouter;
use crate::protocol::{read_message, write_message, ControlMessage, ProtocolError, SidecarEvent};

/// Write side of the sidecar transport. Local mode wraps the child's stdin;
/// remote mode wraps the TCP socket's owned write half. Both implement
/// `AsyncWrite + Unpin` so `write_message` is reused for both.
enum WriterSide {
    Stdin(ChildStdin),
    Tcp(OwnedWriteHalf),
}

/// How a sidecar instance is brought up.
///
/// `Local` spawns a child process (same machine). `Remote` connects to a
/// `sidecar_bridge.py` daemon on another host (e.g. a Jetson) over TCP — the
/// daemon owns the actual sidecar process and bridges its stdin/stdout to the
/// socket. The JSONL protocol is identical either way; only the transport
/// differs.
#[derive(Clone)]
pub enum SpawnConfig {
    /// Spawn the sidecar as a local child process (default; original behavior).
    Local {
        python_bin: String,
        script_path: PathBuf,
        extra_env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    },
    /// Connect to a remote bridge daemon over TCP. The daemon is responsible
    /// for spawning / killing the actual sidecar process when the connection
    /// opens / closes.
    Remote { host: String, port: u16 },
}

impl SpawnConfig {
    /// Materialize one sidecar instance (spawn or connect) + its reader task.
    ///
    /// `router` is an optional EventRouter that the reader_loop uses to publish
    /// each parsed event to the NeoMind EventBus BEFORE queueing it on the
    /// internal channel. None in tests / when the supervisor hasn't been
    /// configured with a router.
    pub async fn spawn(
        &self,
        router: Option<Arc<EventRouter>>,
    ) -> std::io::Result<(SidecarHandle, JoinHandle<()>)> {
        match self {
            Self::Local { python_bin, script_path, extra_env } => {
                SidecarHandle::spawn_with_env(
                    python_bin,
                    script_path,
                    extra_env.iter().cloned(),
                    router,
                )
                .await
            }
            Self::Remote { host, port } => {
                SidecarHandle::connect_remote(host, *port, router).await
            }
        }
    }

    /// Whether this config connects to a remote daemon (vs spawning locally).
    pub fn is_remote(&self) -> bool {
        matches!(self, Self::Remote { .. })
    }
}

/// Handle to a running Python sidecar process.
///
/// Wraps the child process, its stdin (for control messages), and an mpsc receiver
/// that drains events parsed from the child's stdout by a background reader task.
pub struct SidecarHandle {
    /// Child process; `None` in remote mode (the daemon owns the process).
    child: Mutex<Option<Child>>,
    /// The runtime that owns the writer's I/O reactor. Captured at construction
    /// time so `send()` can poll the writer on the correct reactor. In FFI
    /// production this is the persistent runtime (supervisor.start wraps the
    /// spawn in persistent_runtime's context); in tests it's the test runtime.
    runtime: tokio::runtime::Handle,
    /// Write side of the transport. `None` after shutdown (or once stdin/socket
    /// is closed). Locked separately from `child` so heartbeat writes and user
    /// `add_stream` writes don't contend with shutdown's child.wait().
    ///
    /// `Arc<Mutex<...>>` so `send()` can clone the Arc into a task spawned on
    /// `runtime` (the FFI command's ephemeral runtime dies when the command
    /// returns; the writer must be polled on the runtime that owns it).
    writer: Arc<Mutex<Option<WriterSide>>>,
    /// Mutex (not `&mut self`) so both the heartbeat task (Task 2.3) AND user-facing code
    /// can call recv() via shared `&SidecarHandle` references. The Mutex serializes actual
    /// recv calls — mpsc::UnboundedReceiver is single-consumer anyway.
    event_rx: Mutex<mpsc::UnboundedReceiver<SidecarEvent>>,
    /// Dedicated priority channel for `Pong` and `Bye` events (spec §4.6, §4.8.1).
    pong_rx: Mutex<mpsc::UnboundedReceiver<SidecarEvent>>,
    /// Number of health_check pings sent since spawn (Observable for tests + diagnostics).
    ping_count: AtomicU64,
    /// True in remote mode — used by `shutdown()` to skip the SIGTERM/SIGKILL
    /// escalation (no local child to signal; closing the write half tells the
    /// daemon to kill the sidecar).
    is_remote: bool,
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
        Self::spawn_with_env(python_bin, script_path, std::iter::empty(), None).await
    }

    /// Spawn the sidecar with additional environment variables.
    ///
    /// Used by tests that need to enable mock modes (e.g. MOCK_IGNORE_HEALTHCHECK=true
    /// to verify heartbeat timeout behavior without an actual unresponsive process).
    pub async fn spawn_with_env(
        python_bin: &str,
        script_path: &Path,
        extra_env: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
        router: Option<Arc<EventRouter>>,
    ) -> std::io::Result<(Self, tokio::task::JoinHandle<()>)> {
        // NOTE: this function uses bare `tokio::spawn` (NOT persistent_runtime)
        // because callers either (a) wrap us via supervisor.start() — which
        // runs our body on the persistent runtime, so tokio::spawn is
        // persistent — or (b) call us directly inside a #[tokio::test] with
        // `start_paused = true`, where using the test runtime is essential
        // for time-based assertions. Either way the current runtime is the
        // right one.
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
            reader_loop(stdout, event_tx, pong_tx, router).await;
        });

        Ok((
            Self {
                child: Mutex::new(Some(child)),
                runtime: current_or_persistent_handle(),
                writer: Arc::new(Mutex::new(Some(WriterSide::Stdin(stdin)))),
                event_rx: Mutex::new(event_rx),
                pong_rx: Mutex::new(pong_rx),
                ping_count: AtomicU64::new(0),
                is_remote: false,
            },
            reader_task,
        ))
    }

    /// Connect to a remote `sidecar_bridge.py` daemon over TCP. The daemon
    /// owns the actual sidecar process — it spawns one on connection and kills
    /// it when the connection drops. This enables the "NeoMind host on macOS,
    /// sidecar on Jetson" deployment topology (路 C).
    ///
    /// The JSONL protocol over the socket is identical to stdin/stdout; the
    /// reader loop and heartbeat logic are shared with local mode.
    pub async fn connect_remote(
        host: &str,
        port: u16,
        router: Option<Arc<EventRouter>>,
    ) -> std::io::Result<(Self, tokio::task::JoinHandle<()>)> {
        dbg_log(&format!("connect_remote: connecting to {host}:{port}"));
        // See NOTE in spawn_with_env: callers either wrap us via
        // supervisor.start() (so we're already on persistent) or invoke us
        // from a test runtime. Either way, bare tokio primitives are correct.
        let stream = TcpStream::connect((host, port)).await?;
        dbg_log("connect_remote: TCP connected");
        // Disable Nagle — the protocol is request/response JSONL and we want
        // each control message flushed immediately (heartbeats especially).
        let _ = stream.set_nodelay(true);
        let (read_half, write_half) = stream.into_split();

        let (event_tx, event_rx) = mpsc::unbounded_channel::<SidecarEvent>();
        let (pong_tx, pong_rx) = mpsc::unbounded_channel::<SidecarEvent>();
        let reader_task = tokio::spawn(async move {
            dbg_log("reader_loop: started");
            reader_loop(read_half, event_tx, pong_tx, router).await;
            dbg_log("reader_loop: exited");
        });

        Ok((
            Self {
                child: Mutex::new(None),
                runtime: current_or_persistent_handle(),
                writer: Arc::new(Mutex::new(Some(WriterSide::Tcp(write_half)))),
                event_rx: Mutex::new(event_rx),
                pong_rx: Mutex::new(pong_rx),
                ping_count: AtomicU64::new(0),
                is_remote: true,
            },
            reader_task,
        ))
    }

    /// Whether this handle was created via `connect_remote` (vs local spawn).
    pub fn is_remote(&self) -> bool {
        self.is_remote
    }

    /// Send a control message to the sidecar's stdin (local) or TCP write half
    /// (remote). Both paths reuse `write_message` since ChildStdin and
    /// OwnedWriteHalf both implement `AsyncWrite + Unpin`.
    pub async fn send(&self, msg: &ControlMessage) -> Result<(), ProtocolError> {
        // Spawn the write on the runtime that owns the writer's reactor (stored
        // in `self.runtime` at construction time). The FFI command path runs on
        // an ephemeral runtime that gets torn down when the command returns;
        // without this indirection, `write_all` fails with "A Tokio 1.x
        // context was found, but it is being shutdown".
        let writer = self.writer.clone();
        let msg_clone = msg.clone();
        let handle = self.runtime.clone();
        handle
            .spawn(async move {
                let mut guard = writer.lock().await;
                match guard.as_mut() {
                    Some(WriterSide::Stdin(stdin)) => write_message(stdin, &msg_clone).await,
                    Some(WriterSide::Tcp(w)) => write_message(w, &msg_clone).await,
                    None => Err(ProtocolError::Io(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "sidecar write side already closed",
                    ))),
                }
            })
            .await
            .map_err(|join_err| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("runtime task join failed: {join_err}"),
                ))
            })?
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

    /// Graceful shutdown with escalation (spec §4.8.1).
    ///
    /// **Local mode** sequence:
    ///   1. Send `shutdown {graceful_secs: 5}` control message.
    ///   2. Wait up to 5s for `bye` event on the priority channel.
    ///   3. If `bye` received: wait for process exit (≤2s), done.
    ///   4. If timeout: close stdin (backup EOF signal), send SIGTERM, wait 2s.
    ///   5. If still alive: SIGKILL.
    ///
    /// **Remote mode** sequence:
    ///   1. Send `shutdown {graceful_secs: 5}` over TCP.
    ///   2. Wait up to 5s for `bye` (the daemon forwards it from the sidecar's
    ///      stdout before the socket closes).
    ///   3. Drop the write half (half-close). The daemon detects the EOF and
    ///      kills the sidecar itself — no SIGTERM/SIGKILL path because there's
    ///      no local child to signal.
    ///
    /// Returns `Err` only if the underlying wait/kill syscalls fail (not on
    /// timeout — timeout triggers the escalation path which is itself best-effort).
    pub async fn shutdown(&self) -> std::io::Result<()> {
        const GRACEFUL_SECS: u64 = 5;
        const PROCESS_EXIT_SECS: u64 = 2;
        const SIGTERM_WAIT_SECS: u64 = 2;

        // 1. Try to send the shutdown control message. If send fails (broken pipe
        //    because sidecar already died / socket already closed), skip straight
        //    to the escalation path.
        let sent_msg = self
            .send(&ControlMessage::Shutdown { graceful_secs: GRACEFUL_SECS as u32 })
            .await
            .is_ok();

        if sent_msg {
            // 2. Wait up to GRACEFUL_SECS for `bye` on the priority channel.
            if let Ok(Some(SidecarEvent::Bye { .. })) = tokio::time::timeout(
                Duration::from_secs(GRACEFUL_SECS),
                self.recv_pong(),
            ).await {
                if self.is_remote {
                    // Remote: bye received means the sidecar is cleaning up; the
                    // daemon will reap it. Dropping the write half signals the
                    // daemon that we're done and it can release the sidecar.
                    let mut guard = self.writer.lock().await;
                    let _taken = guard.take();
                    return Ok(());
                }
                // Local: bye received — wait for the process to exit (≤2s).
                let mut child_guard = self.child.lock().await;
                if let Some(child) = child_guard.as_mut() {
                    match tokio::time::timeout(Duration::from_secs(PROCESS_EXIT_SECS), child.wait()).await {
                        Ok(Ok(_status)) => return Ok(()),
                        Ok(Err(e)) => return Err(e),
                        Err(_) => { /* fall through to SIGTERM */ }
                    }
                } else {
                    return Ok(());
                }
            }
        }

        // Remote mode stops here — no local process to SIGTERM/SIGKILL. Just
        // drop the write half; the daemon will kill the sidecar on socket EOF.
        if self.is_remote {
            let mut guard = self.writer.lock().await;
            let _taken = guard.take();
            return Ok(());
        }

        // 4. Close the write side as a backup signal (in case the shutdown
        //    message was lost or the sidecar's main loop is stuck before
        //    reading it).
        {
            let mut guard = self.writer.lock().await;
            let _taken = guard.take();
        }

        // 5. SIGTERM escalation (Unix) or direct kill (Windows).
        let mut child_guard = self.child.lock().await;
        let child = match child_guard.as_mut() {
            Some(c) => c,
            None => return Ok(()), // Already taken (shouldn't happen in local mode)
        };
        // Brief wait — process might be exiting from stdin close alone.
        match tokio::time::timeout(Duration::from_millis(500), child.wait()).await {
            Ok(Ok(_)) => return Ok(()),
            Ok(Err(e)) => return Err(e),
            Err(_) => {}
        }

        send_sigterm_or_kill(child).await?;

        // 6. Wait SIGTERM_WAIT_SECS; if still alive, SIGKILL.
        match tokio::time::timeout(Duration::from_secs(SIGTERM_WAIT_SECS), child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                child.kill().await?;
                child.wait().await?;
                Ok(())
            }
        }
    }
}

/// Send SIGTERM on Unix; on Windows there's no SIGTERM so go straight to SIGKILL.
#[cfg(unix)]
async fn send_sigterm_or_kill(child: &mut tokio::process::Child) -> std::io::Result<()> {
    let pid = child.id().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "child has no pid (already reaped?)")
    })?;
    // SAFETY: libc::kill with a real PID and a signal number is safe.
    // Returns 0 on success, -1 on error (errno set).
    let rc = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    if rc != 0 {
        // Fall back to SIGKILL via tokio (always available).
        child.kill().await?;
    }
    Ok(())
}

#[cfg(not(unix))]
async fn send_sigterm_or_kill(child: &mut tokio::process::Child) -> std::io::Result<()> {
    // Windows has no SIGTERM analog; escalate directly to TerminateProcess.
    child.kill().await
}

/// Background task: read JSONL messages from the sidecar's stdout (local mode)
/// or the TCP read half (remote mode) until EOF or error, demuxing each parsed
/// event to either the event channel or the priority channel.
///
/// Generic over `R: AsyncRead + Unpin` so both `ChildStdout` and `OwnedReadHalf`
/// reuse the same loop. Pong and Bye events go to `pong_tx` (consumed by the
/// heartbeat task and the shutdown sequence); everything else goes to `event_tx`
/// (consumed by user-facing recv()). This split means a flood of Detection
/// events cannot starve the heartbeat's pong wait (spec §4.6) or shutdown's bye
/// wait (spec §4.8.1).
///
/// The BufReader is owned here so leftover bytes after a newline are preserved
/// across reads — and on TCP, so a partial JSONL frame split across packets
/// does not corrupt the stream.
async fn reader_loop<R: AsyncRead + Unpin>(
    reader: R,
    event_tx: mpsc::UnboundedSender<SidecarEvent>,
    pong_tx: mpsc::UnboundedSender<SidecarEvent>,
    router: Option<Arc<EventRouter>>,
) {
    let mut reader = tokio::io::BufReader::new(reader);
    loop {
        match read_message::<_, SidecarEvent>(&mut reader).await {
            Ok(ev) => {
                // Route through the EventRouter BEFORE queueing. This publishes
                // the event to the NeoMind EventBus (so the frontend WS
                // receives stats / detection / analytics in real time) while
                // the channel below still feeds command handlers (wait_event,
                // heartbeat). Routing is best-effort: failures are logged
                // inside route() and never break the reader loop.
                if let Some(ref router) = router {
                    let _ = router.route(ev.clone()).await;
                }
                let is_priority = matches!(ev, SidecarEvent::Pong { .. } | SidecarEvent::Bye { .. });
                let tx = if is_priority { &pong_tx } else { &event_tx };
                if tx.send(ev).is_err() {
                    break;
                }
            }
            Err(e) => {
                eprintln!("[deepstream] sidecar reader error: {:?}", e);
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
    /// How to (re)spawn the sidecar — local child process or remote TCP daemon.
    spawn_config: SpawnConfig,
    /// Optional EventRouter passed to every reader_loop so events are published
    /// to the NeoMind EventBus as they arrive. Set via `set_router` before
    /// `start()`. None in unit tests.
    router: Option<Arc<EventRouter>>,
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
    /// Construct a supervisor that spawns the sidecar as a local child process
    /// (original behavior — NeoMind host and sidecar on the same machine).
    pub fn new(python_bin: &str, script_path: PathBuf) -> Self {
        Self::with_config(SpawnConfig::Local {
            python_bin: python_bin.to_string(),
            script_path,
            extra_env: Vec::new(),
        })
    }

    /// Construct a supervisor that connects to a remote `sidecar_bridge.py`
    /// daemon over TCP (路 C — NeoMind host on one machine, sidecar on a
    /// Jetson elsewhere on the LAN).
    pub fn new_remote(host: &str, port: u16) -> Self {
        Self::with_config(SpawnConfig::Remote {
            host: host.to_string(),
            port,
        })
    }

    /// Construct from an explicit [`SpawnConfig`] (covers both modes).
    pub fn with_config(spawn_config: SpawnConfig) -> Self {
        Self {
            spawn_config,
            router: None,
            current: Mutex::new(None),
            restart_count: AtomicU64::new(0),
            restart_history: Mutex::new(Vec::new()),
            state: Mutex::new(SupervisorState::Stopping),
        }
    }

    /// Set the EventRouter used by every reader_loop to publish sidecar events
    /// to the NeoMind EventBus. Call before `start()`. The router is cloned into
    /// each (re)spawn so crash recovery also gets event publishing.
    pub fn set_router(&mut self, router: Arc<EventRouter>) {
        self.router = Some(router);
    }

    /// Add an env var to be passed to every (re)spawn of the sidecar.
    /// Only meaningful in `Local` mode; no-op in `Remote` mode (the daemon
    /// owns the spawn environment).
    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        if let SpawnConfig::Local { extra_env, .. } = &mut self.spawn_config {
            extra_env.push((
                std::ffi::OsString::from(key),
                std::ffi::OsString::from(value),
            ));
        }
        self
    }

    /// Whether this supervisor connects to a remote daemon.
    pub fn is_remote(&self) -> bool {
        self.spawn_config.is_remote()
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
        // Run the entire body on the persistent runtime. The initial spawn
        // and the watch loop must bind to the persistent reactor so they
        // outlive the FFI call. EnterGuard is !Send so we can't hold it
        // across awaits in a Send future; spawning the body sidesteps that.
        let on_restart = Arc::new(on_restart);
        let join = persistent_runtime().handle().spawn(async move {
            // 1. Initial spawn — failure here is fatal and bubbles to caller.
            let (handle, reader_task) = self.spawn_config.spawn(self.router.clone()).await?;
            let handle = Arc::new(handle);
            // Set state=Running BEFORE publishing the handle in `current` so the
            // state transition is complete before any reader_task-exit
            // observation can race a `state()` reader (I1 ordering invariant).
            *self.state.lock().await = SupervisorState::Running;
            *self.current.lock().await = Some(SupervisorEntry {
                handle: handle.clone(),
                reader_task,
            });

            // 2. Launch the watch loop. The callback is wrapped in Arc<F> so
            //    the spawned task can own it (Fn is ?Sized).
            let watch_task = tokio::spawn(watch_loop(self.clone(), on_restart));
            Ok((handle, watch_task))
        });
        join.await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("join: {e}")))?
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
        let (handle, reader_task) = match sup.spawn_config.spawn(sup.router.clone()).await {
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
