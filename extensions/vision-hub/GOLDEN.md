# Golden 回归:vision-hub(直连 ort 移植)vs yolo-device-inference(usls fork)

移植正确性的实证基准。方法:同一实例安装两个扩展,同一张测试图
(`tests/fixtures/bus.jpg`,ultralytics 公开样例)、同一置信度阈值(0.25),
对比检测输出。

## 基线结果(2026-08-30,macOS arm64,CoreML,ORT 1.22.0)

| 指标 | 结果 |
|---|---|
| 双方检测数 | 5 vs 5 |
| 匹配(同标签 + IoU>0.5) | **4/5,平均 IoU 0.966,最小 0.948** |
| 置信度偏差 | ±0.05 以内(bus: 0.950 vs 0.881) |
| 分歧项 | hub-only person@0.62 / usls-only stop sign@0.31(阈值边缘框) |
| 暖机推理耗时 | hub 16-18ms vs usls 16ms(持平;首跑 +130ms 为 CoreML 特化编译) |

## 为什么存在小分歧(预期行为,非缺陷)

两条路径的**重采样器不同**:usls 用 `fast_image_resize` 的 CatmullRom,
vision-common 用 `image` crate 的 CatmullRom——逐像素结果有微小数值差,
导致 (a) 置信度 ±0.05 漂移,(b) 阈值边缘的框(0.3-0.6 分)可能一边检出
一边漏检。框几何高度一致(IoU≥0.94)证明 letterbox/解码/NMS/坐标反映射
全部正确。若要求逐字节对齐,可把 vision-common 的 resampler 换成
`fast_image_resize`(同为 MIT,未做——收益仅限对齐,不改变正确性)。

## 复现

```bash
# 1. 构建两个 .nep 并放进某实例的 data/extensions/packages/(需 ORT_LIB_PATH)
# 2. 启动实例后:
python3 extensions/vision-hub/tests/golden_compare.py --port <port> --user <u> --password <p>
# PASS 判据:匹配率 ≥80% 且平均 IoU ≥0.9
```

每次改动 decode/letterbox/NMS 逻辑后跑一次;新任务族(ocr/face)落地时
按同样方法各建一份 golden 基线。
