//! One-shot frame extraction.
//!
//! Opens a video URL, decodes the **first available frame** (= "latest" for
//! live sources, = first frame for files), encodes it as JPEG, and returns
//! either a base64 string or a file path.
//!
//! FFmpeg calls are blocking, so `handle_command_blocking` must be invoked
//! from `tokio::task::spawn_blocking` (the caller in `lib.rs` does this).
//! A 15s wall-clock timeout wraps the whole decode path so a stuck live
//! source cannot hang the extension.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use neomind_extension_sdk::ExtensionError;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{parse_source_url, FfmpegDecoder, FrameResult};

/// Wall-clock budget for opening the source and decoding one frame.
const DECODE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ExtractFrameParams {
    pub url: String,
    #[serde(default)]
    pub output: OutputMode,
    pub output_path: Option<String>,
    /// If specified, both `width` and `height` must be present (we don't guess
    /// aspect ratio from one side). If both absent, source dimensions are used.
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(default = "default_quality")]
    pub quality: u8,
}

fn default_quality() -> u8 {
    85
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OutputMode {
    Base64,
    File,
}

impl Default for OutputMode {
    fn default() -> Self {
        OutputMode::Base64
    }
}

enum ExtractResult {
    Base64 { data: String, width: u32, height: u32, size_bytes: usize },
    File { path: String, width: u32, height: u32, size_bytes: usize },
}

/// Command entry point. **Blocking** — caller must run on spawn_blocking.
pub(crate) fn handle_command_blocking(args: &Value) -> Result<Value, ExtensionError> {
    let params: ExtractFrameParams = serde_json::from_value(args.clone())
        .map_err(|e| ExtensionError::ExecutionFailed(format!("invalid params: {}", e)))?;

    validate(&params)?;

    let result = run_with_timeout(params)?;

    Ok(format_result(result))
}

fn validate(params: &ExtractFrameParams) -> Result<(), ExtensionError> {
    if params.quality == 0 || params.quality > 100 {
        return Err(ExtensionError::ExecutionFailed(
            "quality must be between 1 and 100".to_string(),
        ));
    }
    match (params.width, params.height) {
        (Some(_), None) | (None, Some(_)) => Err(ExtensionError::ExecutionFailed(
            "specify both width and height, or neither".to_string(),
        )),
        _ => Ok(()),
    }
}

fn format_result(result: ExtractResult) -> Value {
    match result {
        ExtractResult::Base64 { data, width, height, size_bytes } => json!({
            "success": true,
            "width": width,
            "height": height,
            "mime": "image/jpeg",
            "size_bytes": size_bytes,
            "data": data,
        }),
        ExtractResult::File { path, width, height, size_bytes } => json!({
            "success": true,
            "width": width,
            "height": height,
            "path": path,
            "size_bytes": size_bytes,
        }),
    }
}

fn run_with_timeout(params: ExtractFrameParams) -> Result<ExtractResult, ExtensionError> {
    let (tx, rx) = mpsc::channel::<Result<ExtractResult, String>>();
    let builder = thread::Builder::new().name("extract_frame".to_string());
    let handle = builder
        .spawn(move || {
            let _ = tx.send(extract_inner(params));
        })
        .map_err(|e| ExtensionError::ExecutionFailed(format!("spawn failed: {}", e)))?;

    match rx.recv_timeout(DECODE_TIMEOUT) {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(msg)) => Err(ExtensionError::ExecutionFailed(msg)),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(ExtensionError::ExecutionFailed(
            "decode timeout, no frame received within 15s".to_string(),
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = handle.join();
            Err(ExtensionError::ExecutionFailed(
                "decoder thread terminated unexpectedly".to_string(),
            ))
        }
    }
}

fn extract_inner(params: ExtractFrameParams) -> Result<ExtractResult, String> {
    let source_type = parse_source_url(&params.url)?;

    let (target_w, target_h) = match (params.width, params.height) {
        (Some(w), Some(h)) => (w, h),
        _ => {
            let probe = FfmpegDecoder::new(&source_type, 64, 64)
                .map_err(|e| format!("failed to open input: {}", e))?;
            (probe.decoder.width(), probe.decoder.height())
        }
    };

    let mut decoder = FfmpegDecoder::new(&source_type, target_w, target_h)
        .map_err(|e| format!("failed to open input: {}", e))?;

    let frame = match decoder.next_frame() {
        FrameResult::Frame(f) => f,
        FrameResult::EndOfStream => return Err("no decodable frame in stream".to_string()),
        FrameResult::Error(e) => return Err(format!("decode error: {}", e)),
    };

    let jpeg = super::encode_jpeg(&frame.data, frame.width, frame.height, params.quality);
    if jpeg.is_empty() {
        return Err("jpeg encoding produced empty output".to_string());
    }
    let size_bytes = jpeg.len();
    let width = frame.width;
    let height = frame.height;

    match params.output {
        OutputMode::Base64 => {
            let data = base64_encode(&jpeg);
            Ok(ExtractResult::Base64 { data, width, height, size_bytes })
        }
        OutputMode::File => {
            let path = params.output_path.unwrap_or_else(|| {
                let mut p = std::env::temp_dir();
                p.push(format!("neomind-frame-{}.jpg", short_id()));
                p.to_string_lossy().to_string()
            });
            std::fs::write(&path, &jpeg)
                .map_err(|e| format!("failed to write file '{}': {}", path, e))?;
            Ok(ExtractResult::File { path, width, height, size_bytes })
        }
    }
}

/// RFC 4648 standard base64 encoder. Tiny inline impl — avoids pulling a crate
/// for a single call site.
fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn short_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}", nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_params() {
        let v = json!({ "url": "rtsp://example/test" });
        let p: ExtractFrameParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.url, "rtsp://example/test");
        assert_eq!(p.output, OutputMode::Base64);
        assert_eq!(p.quality, 85);
        assert!(p.width.is_none() && p.height.is_none());
    }

    #[test]
    fn parses_file_mode() {
        let v = json!({ "url": "file:///tmp/x.mp4", "output": "file", "quality": 50 });
        let p: ExtractFrameParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.output, OutputMode::File);
        assert_eq!(p.quality, 50);
    }

    #[test]
    fn rejects_zero_quality() {
        let args = json!({ "url": "rtsp://x", "quality": 0 });
        let err = handle_command_blocking(&args).unwrap_err();
        assert!(format!("{:?}", err).contains("quality must be between 1 and 100"));
    }

    #[test]
    fn rejects_101_quality() {
        let args = json!({ "url": "rtsp://x", "quality": 101 });
        let err = handle_command_blocking(&args).unwrap_err();
        assert!(format!("{:?}", err).contains("quality must be between 1 and 100"));
    }

    #[test]
    fn rejects_one_sided_size() {
        let args = json!({ "url": "rtsp://x", "width": 320 });
        let err = handle_command_blocking(&args).unwrap_err();
        assert!(format!("{:?}", err).contains("both width and height"));
    }

    #[test]
    fn rejects_invalid_output_mode() {
        let args = json!({ "url": "rtsp://x", "output": "png" });
        let err = handle_command_blocking(&args).unwrap_err();
        assert!(format!("{:?}", err).contains("invalid params"));
    }

    #[test]
    fn rejects_missing_url() {
        let args = json!({});
        let err = handle_command_blocking(&args).unwrap_err();
        assert!(format!("{:?}", err).contains("invalid params"));
    }

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"M"), "TQ==");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"hello world"), "aGVsbG8gd29ybGQ=");
    }
}
