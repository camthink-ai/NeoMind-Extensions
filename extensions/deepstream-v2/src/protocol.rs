use serde::{Deserialize, Serialize};
use std::io;

pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("line exceeds {MAX_LINE_BYTES} bytes")]
    LineTooLong,
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
}
