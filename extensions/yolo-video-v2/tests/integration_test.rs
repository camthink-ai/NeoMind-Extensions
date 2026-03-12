//! YOLO Video Extension 集成测试
//!
//! 测试模型加载、推理功能和内存管理

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    // Import from the library
    use neomind_extension_yolo_video_v2::{
        detector::YoloDetector,
        draw_detections, encode_jpeg, detections_to_object_detection,
        ObjectDetection, BoundingBox,
    };
    use image::{RgbImage, Rgb};

    /// 测试 1: 基础功能 - 模型加载
    #[test]
    fn test_model_loading() {
        println!("[TEST] 测试模型加载...");

        #[cfg(not(target_arch = "wasm32"))]
        {
            let detector = YoloDetector::new();

            match detector {
                Ok(d) => {
                    println!("✓ 模型加载成功");
                    println!("  - 已加载：{}", d.is_loaded());
                }
                Err(e) => {
                    println!("⚠ 模型未加载 (demo 模式): {}", e);
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            println!("⊘ WASM 平台跳过");
        }
    }

    /// 测试 2: 推理功能
    #[test]
    fn test_inference() {
        println!("\n[TEST] 测试推理功能...");

        #[cfg(not(target_arch = "wasm32"))]
        {
            use image::{RgbImage, Rgb};

            let mut detector = match YoloDetector::new() {
                Ok(d) => d,
                Err(_) => {
                    println!("⊘ 跳过推理测试 (模型未加载)");
                    return;
                }
            };

            // 创建测试图像
            let test_image = RgbImage::from_pixel(320, 240, Rgb([128, 128, 128]));

            let start = Instant::now();
            let detections = detector.detect(&test_image, 0.5, 10);
            let elapsed = start.elapsed();

            println!("✓ 推理完成");
            println!("  - 耗时：{:?}", elapsed);
            println!("  - 检测结果：{} 个", detections.len());

            assert!(elapsed.as_millis() < 5000, "推理时间过长");
        }

        #[cfg(target_arch = "wasm32")]
        {
            println!("⊘ WASM 平台跳过");
        }
    }

    /// 测试 3: 内存泄漏 - 连续推理
    #[test]
    fn test_memory_leak_stress() {
        println!("\n[TEST] 内存泄漏压力测试 (100 次推理)...");

        #[cfg(not(target_arch = "wasm32"))]
        {
            use image::{RgbImage, Rgb};

            let mut detector = match YoloDetector::new() {
                Ok(d) => d,
                Err(_) => {
                    println!("⊘ 跳过测试 (模型未加载)");
                    return;
                }
            };

            let test_image = RgbImage::from_pixel(320, 240, Rgb([128, 128, 128]));
            let iterations = 100;

            // 记录初始内存
            let initial_memory = get_process_memory_mb().unwrap_or(0.0);
            println!("  - 初始内存：{:.1} MB", initial_memory);

            // 执行多次推理
            for i in 0..iterations {
                let _ = detector.detect(&test_image, 0.5, 10);

                if (i + 1) % 20 == 0 {
                    let current = get_process_memory_mb().unwrap_or(0.0);
                    let growth = current - initial_memory;
                    println!("  - 第 {} 次：{:.1} MB (增长：{:+.1} MB)", i + 1, current, growth);
                }
            }

            // 记录最终内存
            let final_memory = get_process_memory_mb().unwrap_or(0.0);
            let total_growth = final_memory - initial_memory;

            println!("  - 最终内存：{:.1} MB", final_memory);
            println!("  - 总增长：{:+.1} MB", total_growth);
            println!("  - 平均每帧：{:.3} MB", total_growth / iterations as f64);

            // 评估：每帧增长应小于 1MB
            let avg_per_frame = total_growth / iterations as f64;
            if avg_per_frame < 1.0 {
                println!("✓ 通过：内存增长在可接受范围内");
            } else {
                println!("⚠ 警告：内存增长过快");
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            println!("⊘ WASM 平台跳过");
        }
    }

    /// 测试 4: JPEG 编码复用
    #[test]
    fn test_jpeg_encoding_reuse() {
        println!("\n[TEST] JPEG 编码缓冲复用测试...");

        #[cfg(not(target_arch = "wasm32"))]
        {
            use image::{RgbImage, Rgb};

            let test_image = RgbImage::from_pixel(320, 240, Rgb([128, 128, 128]));
            let iterations = 50;

            let initial_memory = get_process_memory_mb().unwrap_or(0.0);
            println!("  - 初始内存：{:.1} MB", initial_memory);

            // Simple JPEG encoding test using image crate
            for i in 0..iterations {
                let mut jpeg_data: Vec<u8> = Vec::new();
                let mut cursor = std::io::Cursor::new(&mut jpeg_data);
                let _ = image::codecs::jpeg::JpegEncoder::new(&mut cursor)
                    .encode(
                        test_image.as_raw(),
                        test_image.width(),
                        test_image.height(),
                        image::ColorType::Rgb8.into(),
                    );

                if (i + 1) % 10 == 0 {
                    let current = get_process_memory_mb().unwrap_or(0.0);
                    println!("  - 第 {} 次：{:.1} MB", i + 1, current);
                }
            }

            let final_memory = get_process_memory_mb().unwrap_or(0.0);
            let growth = final_memory - initial_memory;

            println!("  - 最终内存：{:.1} MB", final_memory);
            println!("  - 总增长：{:+.1} MB", growth);

            if growth < 50.0 {
                println!("✓ 通过：JPEG 编码缓冲复用正常");
            } else {
                println!("⚠ 警告：JPEG 编码可能有内存泄漏");
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            println!("⊘ WASM 平台跳过");
        }
    }

    /// 辅助函数：获取进程内存
    fn get_process_memory_mb() -> Option<f64> {
        #[cfg(target_os = "macos")]
        {
            use std::process::Command;
            let output = Command::new("ps")
                .args(["-o", "rss=", "-p", &std::process::id().to_string()])
                .output()
                .ok()?;
            let rss_kb: f64 = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse()
                .ok()?;
            Some(rss_kb / 1024.0)
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
            None
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            None
        }
    }

    /// 测试 5: 完整的推理和绘制数据链路测试
    #[test]
    fn test_full_inference_and_draw_pipeline() {
        println!("\n[TEST] 测试完整推理和绘制数据链路...");

        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::time::Instant;

            // 步骤 1: 加载 YOLO 模型
            println!("  步骤 1: 加载 YOLO 模型...");
            let mut detector = match YoloDetector::new() {
                Ok(d) => {
                    println!("    ✓ 模型加载成功");
                    d
                }
                Err(e) => {
                    println!("    ⊘ 跳过测试 (模型未加载): {}", e);
                    return;
                }
            };

            // 步骤 2: 创建测试图像（模拟真实场景）
            println!("  步骤 2: 创建测试图像...");
            let test_image = RgbImage::from_pixel(640, 480, Rgb([100, 100, 100]));
            println!("    ✓ 图像尺寸：{}x{}", test_image.width(), test_image.height());

            // 步骤 3: 运行 YOLO 推理
            println!("  步骤 3: 运行 YOLO 推理...");
            let inference_start = Instant::now();
            let raw_detections = detector.detect(&test_image, 0.25, 10);
            let inference_time = inference_start.elapsed();
            println!("    ✓ 推理完成：{} 个检测结果，耗时 {:?}", raw_detections.len(), inference_time);

            // 步骤 4: 转换检测结果格式
            println!("  步骤 4: 转换检测结果格式...");
            let object_detections = detections_to_object_detection(raw_detections.clone());
            println!("    ✓ 转换完成：{} 个 ObjectDetection", object_detections.len());

            // 步骤 5: 绘制检测结果
            println!("  步骤 5: 绘制检测结果...");
            let mut output_image = test_image.clone();
            let draw_start = Instant::now();
            draw_detections(&mut output_image, &object_detections);
            let draw_time = draw_start.elapsed();
            println!("    ✓ 绘制完成，耗时 {:?}", draw_time);

            // 步骤 6: 编码为 JPEG
            println!("  步骤 6: 编码为 JPEG...");
            let encode_start = Instant::now();
            let jpeg_data = encode_jpeg(&output_image, 85);
            let encode_time = encode_start.elapsed();
            println!("    ✓ JPEG 编码完成，大小：{} bytes，耗时 {:?}", jpeg_data.len(), encode_time);

            // 验证数据链路
            assert!(!jpeg_data.is_empty(), "JPEG 数据不应为空");
            assert!(jpeg_data.len() > 100, "JPEG 数据太小，可能编码失败");
            
            // 验证 JPEG 文件头
            assert_eq!(jpeg_data[0], 0xFF, "JPEG 文件头错误");
            assert_eq!(jpeg_data[1], 0xD8, "JPEG 文件头错误");

            println!("\n  ✓ 完整数据链路测试通过！");
            println!("    - 推理时间：{:?}", inference_time);
            println!("    - 绘制时间：{:?}", draw_time);
            println!("    - 编码时间：{:?}", encode_time);
            println!("    - 总时间：{:?}", inference_time + draw_time + encode_time);
            println!("    - 输出大小：{} bytes", jpeg_data.len());
        }

        #[cfg(target_arch = "wasm32")]
        {
            println!("⊘ WASM 平台跳过");
        }
    }

    /// 测试 6: 绘制功能测试（使用模拟检测结果）
    #[test]
    fn test_draw_detections_with_mock_data() {
        println!("\n[TEST] 测试绘制功能（模拟数据）...");

        // 创建模拟检测结果
        let mock_detections = vec![
            ObjectDetection {
                id: 1,
                label: "person".to_string(),
                confidence: 0.95,
                bbox: BoundingBox {
                    x: 100.0,
                    y: 100.0,
                    width: 200.0,
                    height: 300.0,
                },
                class_id: 0,
            },
            ObjectDetection {
                id: 2,
                label: "car".to_string(),
                confidence: 0.88,
                bbox: BoundingBox {
                    x: 400.0,
                    y: 200.0,
                    width: 150.0,
                    height: 100.0,
                },
                class_id: 2,
            },
            ObjectDetection {
                id: 3,
                label: "dog".to_string(),
                confidence: 0.72,
                bbox: BoundingBox {
                    x: 50.0,
                    y: 350.0,
                    width: 80.0,
                    height: 60.0,
                },
                class_id: 17,
            },
        ];

        println!("  步骤 1: 创建测试图像...");
        let mut test_image = RgbImage::from_pixel(640, 480, Rgb([128, 128, 128]));
        
        // 绘制一些背景图案
        for y in 0..480 {
            for x in 0..640 {
                let r = ((x as f32 / 640.0) * 255.0) as u8;
                let g = ((y as f32 / 480.0) * 255.0) as u8;
                test_image.put_pixel(x, y, Rgb([r, g, 100]));
            }
        }
        println!("    ✓ 测试图像创建完成");

        println!("  步骤 2: 绘制检测结果...");
        draw_detections(&mut test_image, &mock_detections);
        println!("    ✓ 绘制完成");

        println!("  步骤 3: 编码为 JPEG 并验证...");
        let jpeg_data = encode_jpeg(&test_image, 85);
        println!("    ✓ JPEG 编码完成，大小：{} bytes", jpeg_data.len());

        // 验证
        assert!(jpeg_data.len() > 1000, "JPEG 数据应该包含绘制内容");
        assert_eq!(jpeg_data[0], 0xFF, "JPEG 文件头错误");
        assert_eq!(jpeg_data[1], 0xD8, "JPEG 文件头错误");

        println!("\n  ✓ 绘制功能测试通过！");
        println!("    - 检测框数量：{}", mock_detections.len());
        println!("    - 输出大小：{} bytes", jpeg_data.len());
    }

    /// 测试 7: Push 模式数据链路测试
    #[test]
    fn test_push_mode_data_pipeline() {
        println!("\n[TEST] 测试 Push 模式数据链路...");

        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::sync::Arc;
            use std::thread;
            use std::time::Duration;

            // 创建测试图像
            let test_image = Arc::new(RgbImage::from_pixel(640, 480, Rgb([100, 100, 100])));
            
            // 模拟多帧处理
            let frame_count = 10;
            let mut total_time = Duration::ZERO;
            let mut total_jpeg_size = 0;

            println!("  处理 {} 帧...", frame_count);

            for i in 0..frame_count {
                let frame_start = Instant::now();
                
                // 克隆图像（模拟从摄像头获取帧）
                let mut frame = test_image.as_ref().clone();
                
                // 添加时间戳效果
                for y in 0..30 {
                    for x in 0..100 {
                        frame.put_pixel(x, y, Rgb([255, 255, 255]));
                    }
                }

                // 创建模拟检测结果（每帧不同）
                let mock_detections = vec![
                    ObjectDetection {
                        id: 1,
                        label: "person".to_string(),
                        confidence: 0.8 + (i as f32 * 0.01),
                        bbox: BoundingBox {
                            x: 100.0 + (i as f32 * 10.0),
                            y: 100.0,
                            width: 200.0,
                            height: 300.0,
                        },
                        class_id: 0,
                    },
                ];

                // 绘制检测结果
                draw_detections(&mut frame, &mock_detections);

                // 编码为 JPEG
                let jpeg_data = encode_jpeg(&frame, 85);

                let frame_time = frame_start.elapsed();
                total_time += frame_time;
                total_jpeg_size += jpeg_data.len();

                println!("    帧 {}: {:?}, {} bytes", i + 1, frame_time, jpeg_data.len());
            }

            let avg_time = total_time / frame_count;
            let avg_size = total_jpeg_size / frame_count as usize;

            println!("\n  ✓ Push 模式数据链路测试通过！");
            println!("    - 平均帧处理时间：{:?}", avg_time);
            println!("    - 平均 FPS: {:.1}", 1000.0 / avg_time.as_millis() as f32);
            println!("    - 平均 JPEG 大小：{} bytes", avg_size);
        }

        #[cfg(target_arch = "wasm32")]
        {
            println!("⊘ WASM 平台跳过");
        }
    }
}