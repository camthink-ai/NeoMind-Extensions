//! Temporary perf bench: measures each hot-path stage, old vs new.
//! Run: ORT_DYLIB_PATH=<good dylib> LIBONNXRUNTIME_NO_PKG_CONFIG=1 \
//!      FFMPEG_DIR=/opt/homebrew/opt/ffmpeg@7 cargo run -p yolo-video --example perf_bench

use neomind_extension_yolo_video::{detector::YoloDetector, encode_jpeg, resize_to_640x640};
use image::imageops::FilterType;

fn make_frame(w: u32, h: u32) -> image::RgbImage {
    let mut img = image::RgbImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgb([
            ((x * 7 + y * 13) % 256) as u8,
            ((x * 3 + y * 29) % 256) as u8,
            ((x * 41 + y * 5) % 256) as u8,
        ]);
    }
    img
}

fn bench<F: FnMut()>(name: &str, iters: u32, mut f: F) -> f64 {
    f(); // warmup
    let t = std::time::Instant::now();
    for _ in 0..iters {
        f();
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / iters as f64;
    println!("{:<46} {:>8.2} ms/frame", name, ms);
    ms
}

fn encode_jpeg_old(image: &image::RgbImage, quality: u8) -> Vec<u8> {
    let mut buffer = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, quality);
    let _ = encoder.encode(
        image.as_raw(), image.width(), image.height(),
        image::ExtendedColorType::Rgb8,
    );
    buffer
}

fn main() {
    println!("== yolo-video perf bench (960x540 frames, quality 65) ==\n");

    let frame = make_frame(960, 540);

    // --- 1. Pre-resize: old (image crate Triangle) vs new (fast_image_resize) ---
    let mut fir = fast_image_resize::Resizer::new();
    let resize_old_ms = bench("OLD: pre-resize 640x640 (image/Triangle)", 100, || {
        let _ = image::imageops::resize(&frame, 640, 640, FilterType::Triangle);
    });
    let resize_new_ms = bench("NEW: pre-resize 640x640 (fast_image_resize)", 100, || {
        let _ = resize_to_640x640(&mut fir, &frame);
    });

    // --- 2. JPEG encode: old (image crate) vs new (jpeg-encoder) ---
    let old_size = encode_jpeg_old(&frame, 65).len();
    let new_size = encode_jpeg(&frame, 65).len();
    let jpeg_old_ms = bench("OLD: JPEG encode (image crate, q65)", 100, || {
        let _ = encode_jpeg_old(&frame, 65);
    });
    let jpeg_new_ms = bench("NEW: JPEG encode (jpeg-encoder, q65)", 100, || {
        let _ = encode_jpeg(&frame, 65);
    });
    println!("    (output size: old {} KB, new {} KB)\n", old_size / 1024, new_size / 1024);

    // --- 3. Inference floor: detect on pre-scaled 640x640 ---
    let mut detector = YoloDetector::new().expect("detector");
    detector.ensure_loaded();
    if !detector.is_loaded() {
        eprintln!("model failed to load: {:?}", detector.get_load_error());
        std::process::exit(1);
    }
    let frame640 = make_frame(640, 640);
    let inf_ms = bench("SHARED: detect on 640x640 (inference floor)", 30, || {
        let _ = detector.detect(&frame640, 0.5, 20);
    });

    // --- 4. Composed per-frame cost (excluding decode, which is unchanged) ---
    println!("\n== composed per-frame cost (decode unchanged, not included) ==");
    let old_total = resize_old_ms + inf_ms + jpeg_old_ms;
    let new_total = resize_new_ms + inf_ms + jpeg_new_ms;
    println!("OLD pipeline: {:>6.1} ms/frame  -> {:>5.1} fps ceiling", old_total, 1000.0 / old_total);
    println!("NEW pipeline: {:>6.1} ms/frame  -> {:>5.1} fps ceiling", new_total, 1000.0 / new_total);
    println!("saved: {:.1} ms/frame ({:.0}%)", old_total - new_total, (old_total - new_total) / old_total * 100.0);
}
