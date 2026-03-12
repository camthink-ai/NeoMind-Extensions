//! 独立内存测试程序 - 测试 YOLO 扩展内存泄漏

use std::time::Instant;
use std::process::Command;

fn main() {
    println!("========================================");
    println!("YOLO Extension 内存泄漏测试");
    println!("========================================\n");

    // 测试 1: 模型加载
    println!("[TEST 1] 模型加载测试");
    test_model_load();

    // 测试 2: 连续推理
    println!("\n[TEST 2] 连续推理测试 (100 次)");
    test_inference_stress();

    // 测试 3: JPEG 编码
    println!("\n[TEST 3] JPEG 编码测试 (50 次)");
    test_jpeg_encoding();

    println!("\n========================================");
    println!("测试完成");
    println!("========================================");
}

fn test_model_load() {
    let start = Instant::now();

    #[cfg(not(target_arch = "wasm32"))]
    {
        use neomind_extension_yolo_video_v2::detector::YoloDetector;

        match YoloDetector::new() {
            Ok(detector) => {
                println!("  ✓ 模型加载成功");
                println!("    - 耗时：{:?}", start.elapsed());
                println!("    - 已加载：{}", detector.is_loaded());
            }
            Err(e) => {
                println!("  ⊘ Demo 模式：{}", e);
            }
        }
    }
}

fn test_inference_stress() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use neomind_extension_yolo_video_v2::detector::YoloDetector;
        use image::{RgbImage, Rgb};

        let mut detector = match YoloDetector::new() {
            Ok(d) => d,
            Err(e) => {
                println!("  ⊘ 跳过 (模型未加载): {}", e);
                return;
            }
        };

        let test_image = RgbImage::from_pixel(320, 240, Rgb([128, 128, 128]));
        let iterations = 100;

        let initial_memory = get_memory_mb().unwrap_or(0.0);
        println!("  - 初始内存：{:.1} MB", initial_memory);

        let mut total_time = 0.0;

        for i in 0..iterations {
            let start = Instant::now();
            let _detections = detector.detect(&test_image, 0.5, 10);
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            total_time += elapsed;

            if (i + 1) % 20 == 0 {
                let current = get_memory_mb().unwrap_or(0.0);
                let growth = current - initial_memory;
                println!("  - 第 {:3} 次：{:7.1} MB (增长：{:+.1} MB, 推理：{:.1}ms)",
                    i + 1, current, growth, elapsed);
            }
        }

        let final_memory = get_memory_mb().unwrap_or(0.0);
        let total_growth = final_memory - initial_memory;

        println!("\n  统计:");
        println!("    - 最终内存：{:.1} MB", final_memory);
        println!("    - 总增长：{:+.1} MB", total_growth);
        println!("    - 平均每帧：{:.3} MB", total_growth / iterations as f64);
        println!("    - 平均推理：{:.1} ms", total_time / iterations as f64);

        // 评估
        let avg_per_frame = total_growth / iterations as f64;
        if avg_per_frame < 1.0 {
            println!("  ✓ 通过：内存增长正常 (< 1MB/帧)");
        } else if avg_per_frame < 5.0 {
            println!("  ⚠ 警告：内存增长较快 ({:.1}MB/帧)", avg_per_frame);
        } else {
            println!("  ❌ 失败：检测到内存泄漏 ({:.1}MB/帧)", avg_per_frame);
        }
    }

    #[cfg(target_arch = "wasm32")]
    {
        println!("  ⊘ WASM 跳过");
    }
}

fn test_jpeg_encoding() {
    use image::{RgbImage, Rgb};

    let test_image = RgbImage::from_pixel(320, 240, Rgb([128, 128, 128]));
    let iterations = 50;

    let initial_memory = get_memory_mb().unwrap_or(0.0);
    println!("  - 初始内存：{:.1} MB", initial_memory);

    let mut total_time = 0.0;

    for i in 0..iterations {
        let start = Instant::now();
        
        // Simple JPEG encoding using image crate
        let mut jpeg_data: Vec<u8> = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut jpeg_data);
        let _ = image::codecs::jpeg::JpegEncoder::new(&mut cursor)
            .encode(
                test_image.as_raw(),
                test_image.width(),
                test_image.height(),
                image::ColorType::Rgb8.into(),
            );
        
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        total_time += elapsed;

        if (i + 1) % 10 == 0 {
            let current = get_memory_mb().unwrap_or(0.0);
            println!("  - 第 {:3} 次：{:7.1} MB ({:.1}ms)", i + 1, current, elapsed);
        }
    }

    let final_memory = get_memory_mb().unwrap_or(0.0);
    let growth = final_memory - initial_memory;

    println!("\n  统计:");
    println!("    - 最终内存：{:.1} MB", final_memory);
    println!("    - 总增长：{:+.1} MB", growth);
    println!("    - 平均编码：{:.1} ms", total_time / iterations as f64);

    if growth < 50.0 {
        println!("  ✓ 通过：JPEG 编码正常");
    } else {
        println!("  ⚠ 警告：JPEG 编码内存增长：{:.1} MB", growth);
    }
}

fn get_memory_mb() -> Option<f64> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        let rss_kb: f64 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .ok()?;
        return Some(rss_kb / 1024.0);
    }

    #[cfg(target_os = "linux")]
    {
        use std::fs;
        let status = fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if line.starts_with("VmRSS:") {
                let kb: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
                return Some(kb / 1024.0);
            }
        }
        return None;
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}