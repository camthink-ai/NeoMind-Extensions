//! Overlay drawing — box + label rendering on images. The single copy of
//! what five extensions each shipped (imageproc + ab_glyph + a font file).

use ab_glyph::{Font as AbFont, FontRef, PxScale, ScaleFont};
use image::RgbImage;
use imageproc::drawing::{draw_filled_rect_mut, draw_hollow_rect_mut, draw_text_mut};
use imageproc::rect::Rect;

use crate::types::Detection;

/// The 10-color detection palette shared by the old extensions.
pub const BOX_COLORS: [(u8, u8, u8); 10] = [
    (239, 68, 68), (34, 197, 94), (59, 130, 246), (234, 179, 8), (6, 182, 212),
    (139, 92, 246), (236, 72, 153), (249, 115, 22), (132, 204, 22), (20, 184, 166),
];

/// Embedded Latin font (NotoSans, SIL OFL 1.1). Chinese labels need the SC
/// font — drop `NotoSansSC-Regular.ttf` next to the extension
/// (`fonts/NotoSansSC-Regular.ttf`) and it is picked up automatically.
const FONT_BYTES: &[u8] = include_bytes!("../assets/NotoSans-Regular.ttf");

fn load_font() -> Option<FontRef<'static>> {
    static FONT: std::sync::OnceLock<Option<FontRef<'static>>> = std::sync::OnceLock::new();
    FONT.get_or_init(|| FontRef::try_from_slice(FONT_BYTES).ok()).clone()
}

/// Load the CJK font from disk if present (NEOMIND_EXTENSION_DIR/fonts/ or
/// ./fonts/), for Chinese label text.
fn load_cjk_font() -> Option<FontRef<'static>> {
    static CJK: std::sync::OnceLock<Option<FontRef<'static>>> = std::sync::OnceLock::new();
    CJK.get_or_init(|| {
        let mut candidates = Vec::new();
        if let Ok(dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
            candidates.push(std::path::PathBuf::from(dir).join("fonts").join("NotoSansSC-Regular.ttf"));
        }
        candidates.push(std::path::PathBuf::from("fonts/NotoSansSC-Regular.ttf"));
        let path = candidates.into_iter().find(|p| p.exists())?;
        let bytes = std::fs::read(path).ok()?;
        // Leak intentionally: the font must live for 'static.
        FontRef::try_from_slice(Box::leak(bytes.into_boxed_slice())).ok()
    })
    .clone()
}

fn label_font(text: &str) -> Option<FontRef<'static>> {
    if text.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c)) {
        load_cjk_font().or_else(load_font)
    } else {
        load_font()
    }
}

/// Draw detections onto an RGB image, in place.
pub fn draw_detections(img: &mut RgbImage, detections: &[Detection]) {
    let (img_w, img_h) = img.dimensions();
    for (i, det) in detections.iter().enumerate() {
        let color = BOX_COLORS[i % BOX_COLORS.len()];
        let rgb = image::Rgb(color.into());

        let x = det.bbox.x.max(0.0).min(img_w as f32 - 2.0) as i32;
        let y = det.bbox.y.max(0.0).min(img_h as f32 - 2.0) as i32;
        let w = det.bbox.w.min(img_w as f32 - x as f32 - 1.0) as u32;
        let h = det.bbox.h.min(img_h as f32 - y as f32 - 1.0) as u32;
        if w < 2 || h < 2 {
            continue;
        }

        draw_hollow_rect_mut(img, Rect::at(x, y).of_size(w, h), rgb);
        draw_hollow_rect_mut(
            img,
            Rect::at(x + 1, y + 1).of_size(w.saturating_sub(2), h.saturating_sub(2)),
            rgb,
        );

        let label_text = format!("{} {:.0}%", det.label, det.confidence * 100.0);
        let font_size = if img_w > 1200 {
            24.0
        } else if img_w > 800 {
            18.0
        } else if w > 100 {
            14.0
        } else {
            11.0
        };
        let Some(font) = label_font(&label_text) else { continue };
        let scale = PxScale::from(font_size);
        let scaled = font.as_scaled(scale);

        let mut text_width = 0.0f32;
        for c in label_text.chars() {
            text_width += scaled.h_advance(scaled.glyph_id(c));
        }
        let label_width = (text_width.ceil() as u32 + 12).min(img_w.saturating_sub(x as u32));
        let label_height = font_size as u32 + 8;
        let label_y = if y >= label_height as i32 { y - label_height as i32 } else { y };

        if label_y >= 0 && label_y as u32 + label_height <= img_h {
            draw_filled_rect_mut(img, Rect::at(x, label_y).of_size(label_width, label_height), rgb);
            draw_text_mut(
                img,
                image::Rgb([255, 255, 255]),
                x + 5,
                label_y + 3,
                scale,
                &font,
                &label_text,
            );
        }
    }
}

/// Convenience: decode → draw → re-encode as JPEG bytes.
pub fn annotate_jpeg(image_bytes: &[u8], detections: &[Detection], quality: u8) -> Result<Vec<u8>, crate::image::ImageError> {
    let img = crate::image::decode(image_bytes)?;
    let mut rgb = img.to_rgb8();
    draw_detections(&mut rgb, detections);
    crate::image::encode_jpeg(&image::DynamicImage::ImageRgb8(rgb), quality)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BBox;

    #[test]
    fn draws_without_panicking_and_changes_pixels() {
        let base = RgbImage::from_pixel(320, 240, image::Rgb([0, 0, 0]));
        let mut img = base.clone();
        let dets = vec![Detection {
            label: "person".into(),
            class_id: Some(0),
            confidence: 0.9,
            bbox: BBox::new(10.0, 10.0, 100.0, 80.0),
            attrs: None,
        }];
        draw_detections(&mut img, &dets);
        assert_ne!(img.as_raw(), base.as_raw());
        // border pixel colored
        assert_ne!(img.get_pixel(10, 10), &image::Rgb([0, 0, 0]));
    }

    #[test]
    fn clips_out_of_bounds_boxes() {
        let mut img = RgbImage::from_pixel(64, 64, image::Rgb([0, 0, 0]));
        let dets = vec![Detection {
            label: "x".into(),
            class_id: None,
            confidence: 0.5,
            bbox: BBox::new(-50.0, -50.0, 500.0, 500.0),
            attrs: None,
        }];
        draw_detections(&mut img, &dets); // must not panic
    }

    #[test]
    fn annotate_jpeg_roundtrip() {
        let img = RgbImage::from_pixel(200, 100, image::Rgb([10, 20, 30]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let dets = vec![Detection {
            label: "cat".into(),
            class_id: Some(0),
            confidence: 0.7,
            bbox: BBox::new(5.0, 5.0, 50.0, 40.0),
            attrs: None,
        }];
        let jpeg = annotate_jpeg(&png, &dets, 85).unwrap();
        assert_eq!(crate::image::sniff_mime(&jpeg), "image/jpeg");
    }
}
