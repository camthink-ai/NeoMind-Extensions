//! Drawing module for face recognition results.
//!
//! Renders bounding boxes, name labels, and similarity percentages onto images.
//! Supports CJK characters via a bundled font.

use image::{ImageResult, Rgb, RgbImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_hollow_rect_mut, draw_text_mut};
use imageproc::rect::Rect;

use crate::FaceResult;

/// Green color for recognized faces: `Rgb([34, 197, 94])`.
pub const RECOGNIZED_COLOR: Rgb<u8> = Rgb([34, 197, 94]);

/// Yellow color for unknown faces: `Rgb([234, 179, 8])`.
pub const UNKNOWN_COLOR: Rgb<u8> = Rgb([234, 179, 8]);

/// White color for label text.
const LABEL_TEXT_COLOR: Rgb<u8> = Rgb([255, 255, 255]);

/// JPEG encoding quality.
const JPEG_QUALITY: u8 = 85;

/// Font pixel scale for label text.
const FONT_SCALE: f32 = 14.0;

/// Label background height in pixels.
const LABEL_HEIGHT: u32 = 20;

/// Horizontal padding inside labels.
const LABEL_PAD_X: u32 = 4;

/// Vertical padding inside labels.
const LABEL_PAD_Y: u32 = 3;

// ---------------------------------------------------------------------------
// Font loading via OnceLock
// ---------------------------------------------------------------------------

static FONT_RESULT: std::sync::OnceLock<
    std::result::Result<ab_glyph::FontRef<'static>, ab_glyph::InvalidFont>,
> = std::sync::OnceLock::new();

fn get_font() -> &'static std::result::Result<ab_glyph::FontRef<'static>, ab_glyph::InvalidFont> {
    FONT_RESULT.get_or_init(|| {
        // Placeholder font (DejaVuSans) -- will be replaced with NotoSansSC in Task 8.
        ab_glyph::FontRef::try_from_slice(include_bytes!(
            "../fonts/NotoSansSC-Regular.ttf"
        ))
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Draw face recognition results onto an image.
///
/// Decodes `image_data` (any format supported by the `image` crate), draws
/// bounding boxes and name labels for each [`FaceResult`], then encodes the
/// result as a JPEG and returns the base64-encoded string.
pub fn draw_recognition_results(
    image_data: &[u8],
    results: &[FaceResult],
) -> ImageResult<String> {
    let mut img = image::load_from_memory(image_data)?.to_rgb8();

    let font = get_font();

    for face in results {
        let color = if face.name.is_some() {
            RECOGNIZED_COLOR
        } else {
            UNKNOWN_COLOR
        };

        let bx = face.face_box.x;
        let by = face.face_box.y;
        let bw = face.face_box.width;
        let bh = face.face_box.height;

        // Clamp coordinates to image bounds.
        let x = (bx.max(0.0) as i32).min(img.width() as i32 - 2);
        let y = (by.max(0.0) as i32).min(img.height() as i32 - 2);
        let w = ((bw as i32).min(img.width() as i32 - x - 1)).max(0) as u32;
        let h = ((bh as i32).min(img.height() as i32 - y - 1)).max(0) as u32;

        if w < 2 || h < 2 {
            continue;
        }

        // Draw bounding box (2 px line width).
        draw_hollow_rect_mut(&mut img, Rect::at(x, y).of_size(w, h), color);
        if w > 2 && h > 2 {
            draw_hollow_rect_mut(
                &mut img,
                Rect::at(x + 1, y + 1)
                    .of_size(w.saturating_sub(2), h.saturating_sub(2)),
                color,
            );
        }

        // Build label text.
        let label_text = match (&face.name, &face.similarity) {
            (Some(name), Some(sim)) => format!("{} {:.0}%", name, sim * 100.0),
            (Some(name), None) => name.clone(),
            _ => "未知".to_string(),
        };

        // Draw label background + text.
        if let Ok(font) = font.as_ref() {
            draw_label(&mut img, x, y, w, &label_text, color, font);
        }
    }

    // Encode to JPEG.
    let jpeg_bytes = encode_jpeg(&img, JPEG_QUALITY);
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &jpeg_bytes,
    ))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Draw a label (background rect + text) above or inside the bounding box.
fn draw_label(
    img: &mut RgbImage,
    bx: i32,
    by: i32,
    bw: u32,
    text: &str,
    box_color: Rgb<u8>,
    font: &ab_glyph::FontRef<'_>,
) {
    let scale = ab_glyph::PxScale {
        x: FONT_SCALE,
        y: FONT_SCALE,
    };

    // Estimate label width from text. Use a per-char width heuristic that
    // works for both Latin and wide (CJK) characters.
    let char_count = text.chars().count();
    let label_width = ((char_count as u32) * 9 + LABEL_PAD_X * 2).min(bw);

    // Position label above the box when there is room; otherwise inside.
    let label_y = if by >= LABEL_HEIGHT as i32 {
        by - LABEL_HEIGHT as i32
    } else {
        by
    };

    if label_width < 20 || label_y < 0 {
        return;
    }
    if (label_y as u32) + LABEL_HEIGHT > img.height() {
        return;
    }

    // Semi-transparent background approximation: blend a dark rect.
    let bg_rect = Rect::at(bx, label_y).of_size(label_width, LABEL_HEIGHT);
    draw_filled_rect_mut(img, bg_rect, box_color);

    // White text.
    draw_text_mut(
        img,
        LABEL_TEXT_COLOR,
        bx + LABEL_PAD_X as i32,
        label_y + LABEL_PAD_Y as i32,
        scale,
        font,
        text,
    );
}

/// Encode an `RgbImage` to JPEG bytes at the given quality.
fn encode_jpeg(img: &RgbImage, quality: u8) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
    // Encoding errors are silently ignored -- the buffer is returned as-is.
    let _ = encoder.encode(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgb8,
    );
    buf
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FaceBox, FaceResult};

    /// Helper: create a small 100x100 white test image as JPEG bytes.
    fn make_test_jpeg() -> Vec<u8> {
        let img = RgbImage::from_pixel(100, 100, Rgb([255, 255, 255]));
        let mut buf = Vec::new();
        let mut enc =
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 90);
        enc.encode(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgb8,
        )
        .expect("jpeg encode");
        buf
    }

    #[test]
    fn test_draw_with_known_input_produces_nonempty_base64() {
        let jpeg = make_test_jpeg();
        let results = vec![FaceResult {
            face_box: FaceBox {
                x: 10.0,
                y: 10.0,
                width: 50.0,
                height: 50.0,
                confidence: 0.95,
                landmarks: None,
            },
            name: Some("Zhang".to_string()),
            similarity: Some(0.92),
            face_id: None,
        }];

        let output = draw_recognition_results(&jpeg, &results)
            .expect("draw should succeed");

        assert!(!output.is_empty(), "base64 output must not be empty");
    }

    #[test]
    fn test_output_decodes_as_valid_jpeg() {
        let jpeg = make_test_jpeg();
        let results = vec![FaceResult {
            face_box: FaceBox {
                x: 5.0,
                y: 5.0,
                width: 40.0,
                height: 40.0,
                confidence: 0.9,
                landmarks: None,
            },
            name: Some("Test".to_string()),
            similarity: Some(0.88),
            face_id: None,
        }];

        let b64 = draw_recognition_results(&jpeg, &results)
            .expect("draw should succeed");

        let decoded = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &b64,
        )
        .expect("base64 decode should succeed");

        // The decoded bytes should parse as a valid image.
        let img =
            image::load_from_memory_with_format(&decoded, image::ImageFormat::Jpeg)
                .expect("should decode as JPEG");
        assert_eq!(img.width(), 100);
        assert_eq!(img.height(), 100);
    }

    #[test]
    fn test_green_color_for_recognized_face() {
        let jpeg = make_test_jpeg();
        let results = vec![FaceResult {
            face_box: FaceBox {
                x: 10.0,
                y: 10.0,
                width: 30.0,
                height: 30.0,
                confidence: 0.9,
                landmarks: None,
            },
            name: Some("Alice".to_string()),
            similarity: Some(0.95),
            face_id: None,
        }];

        let b64 = draw_recognition_results(&jpeg, &results)
            .expect("draw should succeed");
        let decoded = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &b64,
        )
        .expect("base64 decode");
        let img = image::load_from_memory_with_format(&decoded, image::ImageFormat::Jpeg)
            .expect("jpeg decode")
            .to_rgb8();

        // The top-left pixel of the bounding box at (10,10) should have the
        // green channel dominant (recognised color = [34, 197, 94]).
        // Due to JPEG compression we allow some tolerance.
        let pixel = img.get_pixel(10, 10);
        assert!(
            pixel[1] > pixel[0] && pixel[1] > pixel[2],
            "expected green-dominant pixel at box corner, got {:?}",
            pixel,
        );
    }

    #[test]
    fn test_yellow_color_for_unknown_face() {
        let jpeg = make_test_jpeg();
        let results = vec![FaceResult {
            face_box: FaceBox {
                x: 10.0,
                y: 10.0,
                width: 30.0,
                height: 30.0,
                confidence: 0.9,
                landmarks: None,
            },
            name: None,
            similarity: None,
            face_id: None,
        }];

        let b64 = draw_recognition_results(&jpeg, &results)
            .expect("draw should succeed");
        let decoded = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &b64,
        )
        .expect("base64 decode");
        let img = image::load_from_memory_with_format(&decoded, image::ImageFormat::Jpeg)
            .expect("jpeg decode")
            .to_rgb8();

        // Unknown color = [234, 179, 8] -- red and green should both be high.
        let pixel = img.get_pixel(10, 10);
        assert!(
            pixel[0] > 100 && pixel[1] > 100,
            "expected yellow-dominant pixel at box corner, got {:?}",
            pixel,
        );
    }
}
