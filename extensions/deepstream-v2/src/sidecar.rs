//! SidecarHandle — supervisor's handle to one Python sidecar process instance.
//!
//! On crash (Task 2.4), the supervisor drops the old handle and creates a new one.
//! The stdout reader task runs for the lifetime of one sidecar. It owns the BufReader
//! so leftover bytes from a multi-event chunk are preserved.
//!
//! Concurrency: stdin/stdout/child are independent locks so heartbeat writes (Task 2.3)
//! can't block user `add_stream` writes.

use std::path::Path;

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
        let mut cmd = tokio::process::Command::new(python_bin);
        cmd.arg(script_path);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::inherit()); // logs to host stderr
        // Kill the child if the handle is dropped — critical for test isolation
        // (a panicking test must not leak a python process).
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");

        let (tx, rx) = mpsc::unbounded_channel::<SidecarEvent>();
        let reader_task = tokio::spawn(async move {
            stdout_reader_loop(stdout, tx).await;
        });

        Ok((
            Self {
                child: Mutex::new(child),
                stdin: Mutex::new(Some(stdin)),
                event_rx: Mutex::new(rx),
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
/// forwarding each parsed event to the channel.
///
/// The BufReader is owned here so leftover bytes after a newline are preserved
/// across reads (the bug that prompted the Part A refactor).
async fn stdout_reader_loop(
    stdout: ChildStdout,
    tx: mpsc::UnboundedSender<SidecarEvent>,
) {
    let mut reader = tokio::io::BufReader::new(stdout);
    loop {
        match read_message::<_, SidecarEvent>(&mut reader).await {
            Ok(ev) => {
                if tx.send(ev).is_err() {
                    // Receiver dropped — SidecarHandle is gone. Stop reading.
                    break;
                }
            }
            Err(e) => {
                eprintln!("[deepstream-v2] sidecar stdout reader error: {:?}", e);
                break;
            }
        }
    }
}
