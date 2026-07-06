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
    Stats {
        ts: i64,
        global_fps: f32,
        gpu_utilization_percent: f32,
        gpu_memory_used_mb: f32,
        per_stream: Vec<StreamStat>,
    },
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
}
