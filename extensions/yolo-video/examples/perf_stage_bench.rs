//! Stage bench: decode → pre-resize → draw → JPEG encode, no model involved.
//! Run: FFMPEG_DIR=/opt/homebrew/opt/ffmpeg@7 \
//!      cargo run --release -p yolo-video --example perf_stage_bench -- /tmp/test_1080p25.mp4

use neomind_extension_yolo_video::{draw_detections, encode_jpeg, resize_to_640x640, video_source};
use neomind_extension_yolo_video::video_source::VideoSource as _;
use neomind_extension_yolo_video::BoundingBox;

use image::imageops::FilterType;

struct Det {
    label: &'static str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "/tmp/test_1080p25.mp4".to_string());

    // --- 1. Decode: H.264 1080p25 → swscale → RGB24 960x540 (+ copy out) ---
    let st = video_source::parse_source_url(&format!("file://{}", path)).expect("parse");
    let mut src = video_source::FfmpegVideoSource::new(&st).expect("open");
    let info = src.info().clone();
    println!("source: {} @ {:.0}fps -> decode out {}x{}", info.codec, info.fps, info.width, info.height);

    let mut frames: Vec<image::RgbImage> = Vec::new();
    let t = std::time::Instant::now();
    let mut n = 0u32;
    loop {
        match src.next_frame() {
            video_source::FrameResult::Frame(vf) => {
                if let Some(img) = vf.to_rgb_image() {
                    if frames.len() < 5 {
                        frames.push(img); // keep a few frames for the later stages
                    }
                }
                n += 1;
            }
            video_source::FrameResult::EndOfStream => break,
            _ => break,
        }
    }
    let decode_ms = t.elapsed().as_secs_f64() * 1000.0 / n.max(1) as f64;
    println!("{:<48} {:>8.2} ms/frame  ({} frames)", "STAGE decode: h264 1080p -> RGB24 960x540", decode_ms, n);

    let frame = &frames[0];

    // --- 2. Pre-resize: old (image/Triangle) vs new (fast_image_resize) ---
    let t = std::time::Instant::now();
    for _ in 0..100 {
        let _ = image::imageops::resize(frame, 640, 640, FilterType::Triangle);
    }
    println!("{:<48} {:>8.2} ms/frame", "STAGE pre-resize 640x640 (OLD image/Triangle)", t.elapsed().as_secs_f64() * 10.0);
    let mut fir = fast_image_resize::Resizer::new();
    resize_to_640x640(&mut fir, frame).expect("fir");
    let t = std::time::Instant::now();
    for _ in 0..100 {
        let _ = resize_to_640x640(&mut fir, frame);
    }
    println!("{:<48} {:>8.2} ms/frame", "STAGE pre-resize 640x640 (NEW fast_image_resize)", t.elapsed().as_secs_f64() * 10.0);

    // --- 3. Draw: 10 boxes + labels on 960x540 ---
    let dets: Vec<Det> = (0..10)
        .map(|i| Det {
            label: if i % 2 == 0 { "person" } else { "car" },
            x: (i as f32) * 88.0,
            y: (i as f32) * 47.0,
            w: 72.0,
            h: 160.0,
        })
        .collect();
    let dets: Vec<_> = dets
        .iter()
        .map(|d| neomind_extension_yolo_video::ObjectDetection {
            id: 0,
            label: d.label.to_string(),
            confidence: 0.87,
            bbox: BoundingBox { x: d.x, y: d.y, width: d.w, height: d.h },
            class_id: if d.label == "person" { 0 } else { 2 },
        })
        .collect();
    let mut canvas = frame.clone();
    draw_detections(&mut canvas, &dets);
    let t = std::time::Instant::now();
    for _ in 0..100 {
        let mut c = frame.clone();
        draw_detections(&mut c, &dets);
    }
    println!("{:<48} {:>8.2} ms/frame", "STAGE draw 10 boxes+labels (incl. frame clone)", t.elapsed().as_secs_f64() * 10.0);
    let t = std::time::Instant::now();
    for _ in 0..100 {
        draw_detections(&mut canvas, &dets);
    }
    println!("{:<48} {:>8.2} ms/frame", "STAGE draw 10 boxes+labels (pure)", t.elapsed().as_secs_f64() * 10.0);

    // --- 4. JPEG encode: old vs new ---
    let t = std::time::Instant::now();
    for _ in 0..100 {
        let mut b = Vec::new();
        let mut e = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut b, 65);
        let _ = e.encode(canvas.as_raw(), canvas.width(), canvas.height(), image::ExtendedColorType::Rgb8);
    }
    println!("{:<48} {:>8.2} ms/frame", "STAGE JPEG encode q65 (OLD image crate)", t.elapsed().as_secs_f64() * 10.0);
    encode_jpeg(&canvas, 65);
    let t = std::time::Instant::now();
    for _ in 0..100 {
        let _ = encode_jpeg(&canvas, 65);
    }
    println!("{:<48} {:>8.2} ms/frame", "STAGE JPEG encode q65 (NEW jpeg-encoder)", t.elapsed().as_secs_f64() * 10.0);

    // --- Composed CPU-side pipeline (no inference) ---
    let cpu_total = decode_ms + 1.0 + 1.4 + 5.2; // decode + new resize + draw + new jpeg (approx)
    println!("\nCPU-side total (decode+resize+draw+encode): ~{:.1} ms/frame", cpu_total);
}
