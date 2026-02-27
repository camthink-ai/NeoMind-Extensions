//! Video Source Abstraction Layer (V2)
//!
//! Supports multiple video stream protocols through a unified interface.
//! Part of the unified NeoMind Extension SDK.

/// Video source information
#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub codec: String,
    pub is_live: bool,
}

/// Frame from video source
#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub timestamp: i64,
    pub frame_number: u64,
}

/// Result of frame read operation
pub enum FrameResult {
    Frame(VideoFrame),
    EndOfStream,
    NotReady,
    Error(String),
}

/// Video source trait
pub trait VideoSource: Send + Sync {
    fn info(&self) -> &SourceInfo;
    fn is_active(&self) -> bool;
}

/// Parse source URL into source type
pub fn parse_source_url(url: &str) -> Result<SourceType, String> {
    if url.starts_with("camera://") {
        let parts = url.split("://").nth(1).unwrap_or("0");
        let device_index = parts.split('?').next()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0);

        Ok(SourceType::Camera {
            device_index,
            width: 640,
            height: 480,
            fps: 30,
        })
    } else if url.starts_with("rtsp://") {
        Ok(SourceType::RTSP {
            url: url.to_string(),
            transport: RtspTransport::Tcp,
            timeout_secs: 10,
        })
    } else if url.starts_with("rtmp://") {
        Ok(SourceType::RTMP {
            url: url.to_string(),
            app: "live".to_string(),
            stream_key: "stream".to_string(),
        })
    } else if url.starts_with("hls://") || url.contains(".m3u8") {
        Ok(SourceType::HLS {
            url: url.to_string(),
            playlist_reload_secs: 5,
        })
    } else if url.starts_with("file://") {
        let path = url[7..].to_string();
        Ok(SourceType::File {
            path,
            loop_: false,
            start_time_secs: 0.0,
        })
    } else if url.starts_with("screen://") {
        let display = url.split("://").nth(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
        Ok(SourceType::Screen {
            display,
            width: 1920,
            height: 1080,
        })
    } else {
        // Default to camera
        Ok(SourceType::Camera {
            device_index: 0,
            width: 640,
            height: 480,
            fps: 30,
        })
    }
}

/// Supported source types
#[derive(Debug, Clone, PartialEq)]
pub enum SourceType {
    Camera { device_index: i32, width: u32, height: u32, fps: u32 },
    RTSP { url: String, transport: RtspTransport, timeout_secs: u64 },
    RTMP { url: String, app: String, stream_key: String },
    HLS { url: String, playlist_reload_secs: u64 },
    File { path: String, loop_: bool, start_time_secs: f32 },
    Screen { display: u32, width: u32, height: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RtspTransport {
    Tcp,
    Udp,
    Auto,
}

/// Factory for creating video sources
pub struct SourceFactory;

impl SourceFactory {
    pub async fn create(source_type: SourceType) -> Result<Box<dyn VideoSource>, String> {
        match source_type {
            SourceType::Camera { .. } => {
                Err("Camera source not yet implemented".to_string())
            }
            SourceType::RTSP { .. } => {
                Err("RTSP source not yet implemented".to_string())
            }
            SourceType::RTMP { .. } => {
                Err("RTMP source not yet implemented".to_string())
            }
            SourceType::HLS { .. } => {
                Err("HLS source not yet implemented".to_string())
            }
            SourceType::File { .. } => {
                Err("File source not yet implemented".to_string())
            }
            SourceType::Screen { .. } => {
                Err("Screen source not yet implemented".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_camera_url() {
        let result = parse_source_url("camera://0");
        assert!(result.is_ok());
        match result.unwrap() {
            SourceType::Camera { device_index, .. } => {
                assert_eq!(device_index, 0);
            }
            _ => panic!("Expected Camera type"),
        }
    }

    #[test]
    fn test_parse_rtsp_url() {
        let result = parse_source_url("rtsp://192.168.1.100:554/stream");
        assert!(result.is_ok());
        match result.unwrap() {
            SourceType::RTSP { url, .. } => {
                assert_eq!(url, "rtsp://192.168.1.100:554/stream");
            }
            _ => panic!("Expected RTSP type"),
        }
    }

    #[test]
    fn test_parse_hls_url() {
        let result = parse_source_url("hls://example.com/live/stream.m3u8");
        assert!(result.is_ok());
        match result.unwrap() {
            SourceType::HLS { url, .. } => {
                assert_eq!(url, "hls://example.com/live/stream.m3u8");
            }
            _ => panic!("Expected HLS type"),
        }
    }

    #[test]
    fn test_parse_file_url() {
        let result = parse_source_url("file:///path/to/video.mp4");
        assert!(result.is_ok());
        match result.unwrap() {
            SourceType::File { path, .. } => {
                assert_eq!(path, "/path/to/video.mp4");
            }
            _ => panic!("Expected File type"),
        }
    }

    #[test]
    fn test_parse_unknown_url_defaults_to_camera() {
        let result = parse_source_url("unknown://test");
        assert!(result.is_ok());
        match result.unwrap() {
            SourceType::Camera { .. } => {}
            _ => panic!("Expected Camera type as default"),
        }
    }
}
