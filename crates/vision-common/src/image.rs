//! Image IO and preprocessing — decode/encode, data-URL handling, device
//! metric value extraction, and the YOLO-style "fit" letterbox.

use base64::Engine;
use image::DynamicImage;

use crate::types::BBox;

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("decode failed: {0}")]
    Decode(String),
    #[error("encode failed: {0}")]
    Encode(String),
    #[error("invalid input: {0}")]
    Invalid(String),
}

/// YOLO letterbox padding value (ultralytics convention, kept for parity
/// with the usls pipeline this replaces).
pub const LETTERBOX_PAD: u8 = 114;

// ---------------------------------------------------------------------------
// Decode / encode
// ---------------------------------------------------------------------------

/// Decode any supported format (JPEG/PNG/BMP/WebP…) from bytes.
pub fn decode(bytes: &[u8]) -> Result<DynamicImage, ImageError> {
    image::load_from_memory(bytes).map_err(|e| ImageError::Decode(e.to_string()))
}

/// Encode as JPEG.
pub fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, ImageError> {
    let mut buf = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
    img.write_with_encoder(enc)
        .map_err(|e| ImageError::Encode(e.to_string()))?;
    Ok(buf)
}

/// `data:image/jpeg;base64,<...>` → (mime, bytes). Errors if malformed.
pub fn decode_data_url(s: &str) -> Result<(String, Vec<u8>), ImageError> {
    let (meta, b64) = s
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(','))
        .ok_or_else(|| ImageError::Invalid("not a data URL".into()))?;
    // Reject non-base64 data URLs (percent-encoded/plain) early — they
    // would otherwise fail as opaque base64 garbage downstream.
    if !meta.split(';').any(|tok| tok.eq_ignore_ascii_case("base64")) {
        return Err(ImageError::Invalid(
            "data URL is not base64-encoded (missing ;base64 marker)".into(),
        ));
    }
    let mime = meta
        .split(';')
        .next()
        .filter(|m| !m.is_empty())
        .unwrap_or("application/octet-stream")
        .to_string();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| ImageError::Invalid(format!("bad base64: {e}")))?;
    Ok((mime, bytes))
}

/// Wrap bytes as a data URL.
pub fn to_data_url(mime: &str, bytes: &[u8]) -> String {
    format!(
        "data:{};base64,{}",
        mime,
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// Decode an image from a command argument: accepts data-URLs and raw base64.
pub fn decode_image_input(input: &str) -> Result<DynamicImage, ImageError> {
    let bytes = if input.starts_with("data:") {
        decode_data_url(input)?.1
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(input.trim())
            .map_err(|e| ImageError::Invalid(format!("bad base64: {e}")))?
    };
    decode(&bytes)
}

/// Guess MIME from magic bytes.
pub fn sniff_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.len() > 8 && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

// ---------------------------------------------------------------------------
// Device metric value extraction (ported from yolo-device-inference)
// ---------------------------------------------------------------------------

/// Extract base64 image data from a device-metric JSON value. Handles:
/// direct strings, `{"String": "..."}` MetricValue wrappers, common field
/// names (`image`/`data`/`value`/`base64`), dotted nested paths, and
/// data-URL stripping. Returns raw base64 (no `data:` prefix).
pub fn extract_image_base64(
    value: Option<&serde_json::Value>,
    nested_path: Option<&str>,
) -> Option<String> {
    let v = value?;

    let target = if let Some(path) = nested_path {
        let mut current = v;
        for part in path.split('.') {
            current = current.get(part)?;
        }
        current
    } else {
        v
    };

    let raw: Option<&str> = target
        .as_str()
        .or_else(|| target.get("String").and_then(|s| s.as_str()))
        .or_else(|| {
            ["image", "data", "value", "base64"]
                .iter()
                .find_map(|f| target.get(f).and_then(|s| s.as_str()))
        });

    let s = raw?;
    if s.starts_with("data:") {
        s.find(',').map(|p| s[p + 1..].to_string())
    } else {
        Some(s.to_string())
    }
}

/// [`extract_image_base64`] + decode into an image, in one step.
pub fn extract_image(
    value: Option<&serde_json::Value>,
    nested_path: Option<&str>,
) -> Result<DynamicImage, ImageError> {
    let b64 = extract_image_base64(value, nested_path)
        .ok_or_else(|| ImageError::Invalid("no image field in metric value".into()))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&b64)
        .map_err(|e| ImageError::Invalid(format!("metric base64: {e}")))?;
    decode(&bytes)
}

// ---------------------------------------------------------------------------
// Letterbox (usls `ResizeMode::FitAdaptive` semantics)
// ---------------------------------------------------------------------------

/// Result of fitting an image into a fixed canvas.
#[derive(Debug, Clone, Copy)]
pub struct FitResult {
    /// Canvas actually used (tw × th).
    pub width: u32,
    pub height: u32,
    /// Scale factor applied: original = processed / scale.
    pub scale: f32,
}

/// YOLO "fit" preprocessing: scale by `min(tw/w, th/h)`, anchor at the
/// TOP-LEFT, pad right/bottom with `LETTERBOX_PAD` (matches usls
/// `FitAdaptive`, which the postprocess un-scale assumes).
pub fn fit_adaptive(
    img: &DynamicImage,
    target_w: u32,
    target_h: u32,
) -> (image::RgbImage, FitResult) {
    let rgb = img.to_rgb8();
    let (w0, h0) = rgb.dimensions();

    if w0 == 0 || h0 == 0 {
        // Degenerate input: return an all-pad canvas — an honest
        // representation of "no image content" at the target size.
        return (
            image::RgbImage::from_pixel(target_w, target_h, image::Rgb([LETTERBOX_PAD; 3])),
            FitResult {
                width: target_w,
                height: target_h,
                scale: 1.0,
            },
        );
    }
    if (w0, h0) == (target_w, target_h) {
        return (
            rgb,
            FitResult {
                width: target_w,
                height: target_h,
                scale: 1.0,
            },
        );
    }

    let r = (target_w as f32 / w0 as f32).min(target_h as f32 / h0 as f32);
    // Extreme aspect ratios can round to 0 — clamp to >=1 pixel.
    let new_w = ((w0 as f32 * r).round() as u32).clamp(1, target_w);
    let new_h = ((h0 as f32 * r).round() as u32).clamp(1, target_h);

    let resized = image::imageops::resize(&rgb, new_w, new_h, image::imageops::FilterType::CatmullRom);
    let mut canvas = image::RgbImage::from_pixel(target_w, target_h, image::Rgb([LETTERBOX_PAD; 3]));
    image::imageops::overlay(&mut canvas, &resized, 0, 0);

    (
        canvas,
        FitResult {
            width: target_w,
            height: target_h,
            scale: r,
        },
    )
}

/// Convert an RGB canvas to NCHW f32 (RGB order, /255).
pub fn rgb_to_nchw_f32(img: &image::RgbImage) -> Vec<f32> {
    let (w, h) = img.dimensions();
    let mut out = vec![0f32; 3 * (w as usize * h as usize)];
    for (i, px) in img.pixels().enumerate() {
        let [r, g, b] = px.0;
        out[i] = r as f32 / 255.0;
        out[(w as usize * h as usize) + i] = g as f32 / 255.0;
        out[2 * (w as usize * h as usize) + i] = b as f32 / 255.0;
    }
    out
}

/// Map a bbox from letterboxed-input coordinates back to original image
/// coordinates (un-scale + clamp).
pub fn unscale_bbox(bbox: BBox, scale: f32, orig_w: u32, orig_h: u32) -> BBox {
    if scale <= 0.0 {
        return bbox;
    }
    BBox::from_xyxy(
        bbox.xmin() / scale,
        bbox.ymin() / scale,
        bbox.xmax() / scale,
        bbox.ymax() / scale,
    )
    .clamp_to(orig_w, orig_h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView as _;
    use image::RgbImage;

    fn tiny_png() -> Vec<u8> {
        let img = RgbImage::from_pixel(4, 4, image::Rgb([200, 10, 30]));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn decode_encode_roundtrip() {
        let img = decode(&tiny_png()).unwrap();
        assert_eq!(img.dimensions(), (4, 4));
        let jpg = encode_jpeg(&img, 80).unwrap();
        assert_eq!(sniff_mime(&jpg), "image/jpeg");
        let back = decode(&jpg).unwrap();
        assert_eq!(back.dimensions(), (4, 4));
    }

    #[test]
    fn data_url_rejects_non_base64() {
        assert!(decode_data_url("data:,hello").is_err());
        assert!(decode_data_url("data:text/plain,hello").is_err());
        // 空 mime 回退
        let (_, bytes) = decode_data_url("data:;base64,aGk=").unwrap();
        assert_eq!(bytes, b"hi");
    }

    #[test]
    fn data_url_roundtrip() {
        let bytes = tiny_png();
        let url = to_data_url("image/png", &bytes);
        let (mime, decoded) = decode_data_url(&url).unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(decoded, bytes);
        assert!(decode_data_url("not-a-data-url").is_err());
    }

    #[test]
    fn decode_image_input_raw_base64() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(tiny_png());
        let img = decode_image_input(&b64).unwrap();
        assert_eq!(img.dimensions(), (4, 4));
    }

    #[test]
    fn extract_image_variants() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(tiny_png());
        // direct string with data URL
        let v = serde_json::json!(format!("data:image/png;base64,{b64}"));
        assert_eq!(extract_image_base64(Some(&v), None).unwrap(), b64);
        // MetricValue wrapper
        let v = serde_json::json!({ "String": b64 });
        assert_eq!(extract_image_base64(Some(&v), None).unwrap(), b64);
        // common field
        let v = serde_json::json!({ "image": b64, "ts": 123 });
        assert_eq!(extract_image_base64(Some(&v), None).unwrap(), b64);
        // nested path
        let v = serde_json::json!({ "payload": { "value": b64 } });
        assert_eq!(
            extract_image_base64(Some(&v), Some("payload.value")).unwrap(),
            b64
        );
        // nothing
        assert_eq!(extract_image_base64(Some(&serde_json::json!(42)), None), None);
        assert_eq!(extract_image_base64(None, None), None);
        // full decode
        let v = serde_json::json!({ "String": b64 });
        assert_eq!(extract_image(Some(&v), None).unwrap().dimensions(), (4, 4));
    }

    #[test]
    fn fit_adaptive_scales_and_pads() {
        // 8x4 image into 640x640 → r = min(80, 160) = 80 → 640x320, bottom pad
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 4, image::Rgb([255, 0, 0])));
        let (canvas, fit) = fit_adaptive(&img, 640, 640);
        assert_eq!((fit.width, fit.height), (640, 640));
        assert!((fit.scale - 80.0).abs() < 1e-4);
        // top-left pixel is image (red), bottom is pad
        assert_eq!(canvas.get_pixel(0, 0), &image::Rgb([255, 0, 0]));
        assert_eq!(canvas.get_pixel(0, 400), &image::Rgb([114, 114, 114]));
    }

    #[test]
    fn fit_adaptive_tall_image() {
        // 4x8 into 640x640 → r = min(160, 80) = 80 → 320x640, right pad
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(4, 8, image::Rgb([0, 255, 0])));
        let (canvas, fit) = fit_adaptive(&img, 640, 640);
        assert!((fit.scale - 80.0).abs() < 1e-4);
        assert_eq!(canvas.get_pixel(0, 0), &image::Rgb([0, 255, 0]));
        assert_eq!(canvas.get_pixel(400, 0), &image::Rgb([114, 114, 114]));
    }

    #[test]
    fn fit_adaptive_noop_when_same_size() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(640, 640, image::Rgb([1, 2, 3])));
        let (_, fit) = fit_adaptive(&img, 640, 640);
        assert_eq!(fit.scale, 1.0);
    }

    #[test]
    fn nchw_layout_and_unscale() {
        let img = RgbImage::from_pixel(2, 1, image::Rgb([255, 128, 0]));
        let nchw = rgb_to_nchw_f32(&img);
        assert_eq!(nchw.len(), 6);
        assert_eq!(nchw[0], 1.0); // R plane first
        assert!((nchw[2] - 128.0 / 255.0).abs() < 1e-6); // G plane second
        assert_eq!(nchw[4], 0.0); // B plane third

        let scaled = BBox::new(80.0, 160.0, 160.0, 160.0); // in 640-space, scale=80
        let orig = unscale_bbox(scaled, 80.0, 8, 4);
        assert_eq!(orig, BBox::new(1.0, 2.0, 2.0, 2.0));
    }
}
