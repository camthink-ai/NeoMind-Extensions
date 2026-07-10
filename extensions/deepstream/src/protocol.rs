use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;
use std::io;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("line exceeds {MAX_LINE_BYTES} bytes")]
    LineTooLong,
    #[error("invalid utf-8 in line")]
    InvalidUtf8,
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlMessage {
    Hello {
        rtsp_port: u16,
        snapshot_port: u16,
        log_level: String,
        models_dir: String,
        max_streams: u32,
        snapshot_bind_addr: String,
    },
    AddStream { id: String, config: serde_json::Value },
    RemoveStream { id: String, stream_id: String, graceful_secs: u32 },
    UpdateAnalytics { id: String, stream_id: String, line_crossing: serde_json::Value, roi: serde_json::Value },
    SetThreshold { id: String, stream_id: String, conf: f32, iou: f32 },
    ListState { id: String },
    HealthCheck { ts: i64 },
    Shutdown { graceful_secs: u32 },
}

pub fn serialize_line(msg: &ControlMessage) -> Result<String, ProtocolError> {
    let mut s = serde_json::to_string(msg)?;
    if s.len() > MAX_LINE_BYTES { return Err(ProtocolError::LineTooLong); }
    s.push('\n');
    Ok(s)
}

pub fn deserialize_line(line: &str) -> Result<ControlMessage, ProtocolError> {
    if line.len() > MAX_LINE_BYTES { return Err(ProtocolError::LineTooLong); }
    Ok(serde_json::from_str(line)?)
}

pub async fn write_message<W: AsyncWrite + Unpin>(
    w: &mut W,
    msg: &ControlMessage,
) -> Result<(), ProtocolError> {
    let mut line = serde_json::to_string(msg)?;
    if line.len() > MAX_LINE_BYTES {
        return Err(ProtocolError::LineTooLong);
    }
    line.push('\n');
    w.write_all(line.as_bytes()).await.map_err(ProtocolError::Io)?;
    Ok(())
}

pub async fn read_message<R: AsyncRead + Unpin, T: DeserializeOwned>(
    reader: &mut tokio::io::BufReader<R>,
) -> Result<T, ProtocolError> {
    use tokio::io::AsyncBufReadExt;
    loop {
        let mut buf: Vec<u8> = Vec::with_capacity(4096);
        // read_until properly preserves BufReader internal state across
        // calls (unlike manual read() which loses bytes after the newline).
        let n = reader.read_until(b'\n', &mut buf).await.map_err(ProtocolError::Io)?;
        if n == 0 {
            // EOF — underlying reader closed. This is a real end-of-stream,
            // not a blank line in the middle of the banner.
            return Err(ProtocolError::Io(std::io::ErrorKind::UnexpectedEof.into()));
        }
        if buf.len() > MAX_LINE_BYTES {
            return Err(ProtocolError::LineTooLong);
        }

        // Strip the trailing newline (read_until includes it).
        if buf.last() == Some(&b'\n') {
            buf.pop();
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
        }

        // Skip empty lines and non-JSON lines. The DeepStream sidecar runs
        // inside the NVIDIA NGC container whose entrypoint prints a CUDA
        // banner (`\n==========\n== CUDA ==\n==========\n...`) before
        // deepstream_runner.py emits its first JSONL frame. Any of those
        // banner lines would fail serde_json::from_str and previously
        // terminated the reader_loop, causing the extension to fail
        // "sidecar stdout closed before Ready" on every remote-mode spawn.
        // Real JSONL messages start with `{`; banner lines never do.
        let first_non_ws = buf.iter().copied().find(|&b| !b.is_ascii_whitespace());
        if first_non_ws != Some(b'{') {
            let preview = std::str::from_utf8(&buf)
                .unwrap_or("<non-utf8>")
                .chars()
                .take(80)
                .collect::<String>();
            eprintln!(
                "[deepstream] skipping non-JSON preamble line ({}B): {:?}",
                buf.len(),
                preview
            );
            continue;
        }

        let s = std::str::from_utf8(&buf).map_err(|_| ProtocolError::InvalidUtf8)?;
        return serde_json::from_str(s).map_err(Into::into);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SidecarEvent {
    Ready {
        ds_ver: String,
        pyds_ver: String,
        protocol_ver: u32,
        gpu_info: GpuInfo,
    },
    HelloAck {
        max_streams: u32,
        rtsp_url_prefix: String,
        models_loaded: Vec<String>,
    },
    StreamAdded {
        id: String,
        stream_id: String,
        rtsp_url: String,
        snapshot_token: String,
    },
    StreamRemoved {
        id: String,
        stream_id: String,
    },
    StreamError {
        stream_id: String,
        code: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    Detection {
        stream_id: String,
        ts: i64,
        frame_id: u64,
        objects: Vec<DetectionObject>,
    },
    LineCross {
        stream_id: String,
        ts: i64,
        line_id: String,
        track_id: u32,
        class: u32,
        direction: String,
    },
    ROIIntrusion {
        stream_id: String,
        ts: i64,
        roi_id: String,
        track_id: u32,
        class: u32,
        mode: String,
    },
    AnalyticsSnapshot {
        stream_id: String,
        ts: i64,
        snapshot: serde_json::Value,
    },
    Stats(Stats),
    Pong { ts: i64 },
    ErrorResponse {
        id: String,
        code: String,
        message: String,
    },
    Bye {
        reason: String,
        exit_code: i32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub mem_mb: u32,
}

/// Handshake ack fields extracted from [`SidecarEvent::HelloAck`] so callers
/// (e.g. `perform_handshake`) can consume them without pattern-matching the
/// whole enum. Mirrors the variant's payload.
#[derive(Debug, Clone)]
pub struct HelloAck {
    pub max_streams: u32,
    pub rtsp_url_prefix: String,
    pub models_loaded: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionObject {
    pub class: u32,
    pub conf: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<u32>,
    pub bbox: [f32; 4], // left, top, right, bottom — Python decides normalization
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamStat {
    pub stream_id: String,
    pub fps: f32,
    pub latency_ms: f32,
    pub frame_count: u64,
    pub object_count: u32,
    pub status: String,
}

/// Sidecar-wide stats snapshot. Carried by [`SidecarEvent::Stats`].
///
/// `global_fps` is the aggregate throughput across all streams; it surfaces
/// as the `total_throughput_fps` metric on the host side (the rename reflects
/// the metric's display semantics, not a semantic change to the data).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub ts: i64,
    pub global_fps: f32,
    pub gpu_utilization_percent: f32,
    pub gpu_memory_used_mb: f32,
    pub per_stream: Vec<StreamStat>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_add_stream_minimal() {
        let msg = ControlMessage::AddStream {
            id: "r1".into(),
            config: serde_json::json!({"source":{"type":"rtsp","url":"rtsp://x"},"model":"yolov8n-coco"}),
        };
        let line = serialize_line(&msg).unwrap();
        assert!(line.matches('\n').count() == 1 && line.ends_with('\n'));  // single JSON line + newline terminator
        let parsed: ControlMessage = serde_json::from_str(line.trim()).unwrap();
        match parsed { ControlMessage::AddStream { id, .. } => assert_eq!(id, "r1"), _ => panic!() }
    }

    #[test]
    fn reject_line_over_4mb() {
        let huge = "x".repeat(4 * 1024 * 1024 + 1);
        let err = deserialize_line(&huge).unwrap_err();
        assert!(matches!(err, ProtocolError::LineTooLong));
    }

    #[test]
    fn parse_ready_event() {
        let line = r#"{"type":"ready","ds_ver":"7.1.0","pyds_ver":"1.1.1","protocol_ver":1,"gpu_info":{"name":"Orin NX","mem_mb":8192}}"#;
        let ev: SidecarEvent = serde_json::from_str(line).unwrap();
        match ev {
            SidecarEvent::Ready { ds_ver, protocol_ver, .. } => {
                assert_eq!(ds_ver, "7.1.0");
                assert_eq!(protocol_ver, 1);
            }
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn round_trip_three_messages_via_duplex() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut tx, mut rx) = tokio::io::duplex(8 * 1024);
        let msgs = vec![
            ControlMessage::AddStream {
                id: "r1".into(),
                config: serde_json::json!({"source":{"type":"rtsp","url":"rtsp://a"}}),
            },
            ControlMessage::RemoveStream {
                id: "r2".into(),
                stream_id: "r1".into(),
                graceful_secs: 2,
            },
            ControlMessage::HealthCheck { ts: 12345 },
        ];

        let writer = tokio::spawn(async move {
            for m in &msgs {
                write_message(&mut tx, m).await.unwrap();
            }
            tx.shutdown().await.unwrap();
        });

        let mut buf = Vec::new();
        rx.read_to_end(&mut buf).await.unwrap();
        writer.await.unwrap();

        let text = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 3);
        let parsed: Vec<ControlMessage> = lines.iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(matches!(parsed[0], ControlMessage::AddStream { .. }));
        assert!(matches!(parsed[1], ControlMessage::RemoveStream { .. }));
        assert!(matches!(parsed[2], ControlMessage::HealthCheck { .. }));
    }

    #[tokio::test]
    async fn write_message_appends_newline() {
        use tokio::io::AsyncReadExt;
        let (mut tx, mut rx) = tokio::io::duplex(1024);
        write_message(&mut tx, &ControlMessage::HealthCheck { ts: 1 }).await.unwrap();
        tx.shutdown().await.unwrap();
        let mut buf = Vec::new();
        rx.read_to_end(&mut buf).await.unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.ends_with('\n'));
        assert_eq!(s.matches('\n').count(), 1);
    }

    #[tokio::test]
    async fn read_message_round_trip_sidecar_event() {
        use tokio::io::{AsyncWriteExt, BufReader};
        let (mut tx, rx) = tokio::io::duplex(8 * 1024);
        let event_line = r#"{"type":"pong","ts":999}"#;
        tx.write_all(event_line.as_bytes()).await.unwrap();
        tx.write_all(b"\n").await.unwrap();

        let mut reader = BufReader::new(rx);
        let ev = read_message::<_, SidecarEvent>(&mut reader).await.unwrap();
        match ev {
            SidecarEvent::Pong { ts } => assert_eq!(ts, 999),
            _ => panic!("expected Pong"),
        }
    }

    #[tokio::test]
    async fn read_message_rejects_line_over_4mb() {
        use tokio::io::{AsyncWriteExt, BufReader};
        let (mut tx, rx) = tokio::io::duplex(8 * 1024 * 1024);
        let huge = format!("{}\n", "x".repeat(4 * 1024 * 1024 + 1));
        tx.write_all(huge.as_bytes()).await.unwrap();
        tx.shutdown().await.unwrap();

        let mut reader = BufReader::new(rx);
        let err = read_message::<_, SidecarEvent>(&mut reader).await.unwrap_err();
        assert!(matches!(err, ProtocolError::LineTooLong), "got {:?}", err);
    }

    /// Reproduces the bridge-mode bug where the NVIDIA NGC container entrypoint
    /// prints a CUDA banner before the sidecar emits its first JSONL `ready`.
    /// The original read_message would parse the first banner line, fail
    /// serde_json::from_str, and reader_loop would terminate — surfacing as
    /// "sidecar stdout closed before Ready" in perform_handshake. The fix
    /// skips any line whose first non-whitespace byte isn't `{`.
    #[tokio::test]
    async fn read_message_skips_ngc_container_banner() {
        use tokio::io::{AsyncWriteExt, BufReader};
        let banner = b"\n==========\n== CUDA ==\n==========\n\nCUDA Version 12.6.11\n\nContainer image Copyright (c) 2016-2023, NVIDIA CORPORATION & AFFILIATES.\n";
        let ready = br#"{"type":"ready","ds_ver":"7.1","pyds_ver":"unknown","protocol_ver":1,"gpu_info":{"name":"X","mem_mb":1024}}
"#;
        let (mut tx, rx) = tokio::io::duplex(64 * 1024);
        tx.write_all(banner).await.unwrap();
        tx.write_all(ready).await.unwrap();
        tx.shutdown().await.unwrap();

        let mut reader = BufReader::new(rx);
        let ev = read_message::<_, SidecarEvent>(&mut reader).await.expect("must skip banner and parse ready");
        match ev {
            SidecarEvent::Ready { ds_ver, .. } => assert_eq!(ds_ver, "7.1"),
            other => panic!("expected Ready, got {:?}", other),
        }
    }

    /// Plain empty lines (just `\n`) should be skipped, not treated as EOF.
    #[tokio::test]
    async fn read_message_skips_blank_lines() {
        use tokio::io::{AsyncWriteExt, BufReader};
        let (mut tx, rx) = tokio::io::duplex(8 * 1024);
        tx.write_all(b"\n\n\n").await.unwrap();
        tx.write_all(br#"{"type":"pong","ts":42}"#).await.unwrap();
        tx.write_all(b"\n").await.unwrap();
        tx.shutdown().await.unwrap();

        let mut reader = BufReader::new(rx);
        let ev = read_message::<_, SidecarEvent>(&mut reader).await.expect("must skip blanks");
        match ev {
            SidecarEvent::Pong { ts } => assert_eq!(ts, 42),
            other => panic!("expected Pong, got {:?}", other),
        }
    }
}
