// frame.rs — `application/x-neomind-frame` binary container.
//
// The WS push channel carries one frame per message as RAW bytes:
//   [meta_len: u32 (big endian)][meta: JSON utf8][blob: JPEG bytes]
//
// The meta JSON is the old `{img_b64, ts_ns, tracks, tracks_ts, faces,
// present_count}` bundle minus `img_b64` — the JPEG moves into the blob
// section, eliminating BOTH base64 layers on the browser leg (the device-side
// encode is decoded once here at ingest; the platform re-encode never sees
// bytes at all once the session negotiates binary frames).
//
// The platform wraps this container in its 14-byte binary push header; this
// module only owns the app-level split. The browser-side parser
// (GymVideoOverlay.tsx) mirrors `build_frame_payload`/`parse_frame_payload`.

/// data_type reported on the PushOutputMessage riding this container.
pub const FRAME_DATA_TYPE: &str = "application/x-neomind-frame";

/// data_type for hardware-H.264 relay frames (`video/avc`). Same container
/// format; the blob is one Annex-B access unit, the meta carries decoder
/// hints (pts_ns/key/w/h) plus the usual track bundle fields.
pub const AVC_DATA_TYPE: &str = "video/avc";

/// Encode meta + blob into one payload.
pub fn build_frame_payload(meta: &serde_json::Value, blob: &[u8]) -> Vec<u8> {
    // Serialization of a plain object cannot fail; empty meta is the safe
    // fallback (the browser drops the frame, never crashes).
    let meta_bytes = serde_json::to_vec(meta).unwrap_or_else(|_| b"{}".to_vec());
    let mut payload = Vec::with_capacity(4 + meta_bytes.len() + blob.len());
    payload.extend_from_slice(&(meta_bytes.len() as u32).to_be_bytes());
    payload.extend_from_slice(&meta_bytes);
    payload.extend_from_slice(blob);
    payload
}

/// Decode a payload back into (meta, blob). `None` on malformed input —
/// callers drop the frame. Mirrors the browser parser field-for-field.
pub fn parse_frame_payload(payload: &[u8]) -> Option<(serde_json::Value, &[u8])> {
    if payload.len() < 4 {
        return None;
    }
    let meta_len = u32::from_be_bytes(payload[..4].try_into().ok()?) as usize;
    let meta_end = 4 + meta_len;
    if meta_end > payload.len() {
        return None;
    }
    let meta = serde_json::from_slice(&payload[4..meta_end]).ok()?;
    Some((meta, &payload[meta_end..]))
}

/// Standard padded base64 — the REST `get_frame` fallback still serves
/// `img_b64` strings (only the WS push leg is binary).
pub fn encode_b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Decode the device's `img_b64` (Python `base64.b64encode` — standard,
/// padded; tolerant of stray padding differences). Returns `None` unless the
/// bytes look like a JPEG so corrupt frames die here, at the edge.
pub fn decode_b64_jpeg(s: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let trimmed = s.trim();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .ok()
        .or_else(|| {
            base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(trimmed.trim_end_matches('='))
                .ok()
        })?;
    // JPEG SOI marker — cheap sanity gate before the bytes travel further.
    if bytes.len() < 2 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_roundtrip() {
        let meta = serde_json::json!({
            "ts_ns": 123u64,
            "tracks": [{"track_id": 1}],
            "tracks_ts": 100u64,
            "faces": [],
            "present_count": 1,
        });
        let blob: Vec<u8> = vec![0xFF, 0xD8, 0x00, 0x01, 0x02];
        let payload = build_frame_payload(&meta, &blob);

        let (meta2, blob2) = parse_frame_payload(&payload).expect("roundtrip must decode");
        assert_eq!(meta2, meta);
        assert_eq!(blob2, &blob[..]);
    }

    #[test]
    fn payload_empty_blob_roundtrip() {
        let meta = serde_json::json!({"ts_ns": 0u64});
        let payload = build_frame_payload(&meta, &[]);
        let (meta2, blob2) = parse_frame_payload(&payload).unwrap();
        assert_eq!(meta2, meta);
        assert!(blob2.is_empty());
    }

    #[test]
    fn payload_malformed_rejected() {
        // Too short for the length prefix
        assert!(parse_frame_payload(&[0, 0]).is_none());
        // meta_len pointing past the end
        assert!(parse_frame_payload(&[0, 0, 0, 200, b'{', b'}']).is_none());
        // meta not valid JSON
        assert!(parse_frame_payload(&[0, 0, 0, 2, b'x', b'y', 0xFF]).is_none());
    }

    #[test]
    fn b64_jpeg_decode_accepts_padded_and_rejects_garbage() {
        let jpeg = vec![0xFF, 0xD8, 0xE0, 0x00];
        let enc = encode_b64(&jpeg);
        assert_eq!(decode_b64_jpeg(&enc).unwrap(), jpeg);
        // Unpadded variant still decodes (tolerant edge)
        let unpadded = enc.trim_end_matches('=');
        assert!(decode_b64_jpeg(unpadded).is_some());
        // Valid base64 of non-JPEG bytes → rejected by the SOI gate
        assert!(decode_b64_jpeg(&encode_b64(b"not a jpeg")).is_none());
        // Not base64 at all
        assert!(decode_b64_jpeg("!!!").is_none());
    }
}
