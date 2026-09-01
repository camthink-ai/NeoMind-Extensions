# Third-Party Licenses

This crate and the extensions built on it redistribute the following
third-party material. Each component's license terms apply to it.

---

## usls (MIT) — YOLO/OCR decode logic

`src/models/yolo.rs` (and the upcoming `db`/`svtr` OCR ports) derive from
the usls crate by Jamjamjon, vendored as `patches/usls` in this repository
and ported into `vision-common` with behavioral parity (see
`extensions/vision-hub/GOLDEN.md`).

> MIT License
>
> Copyright (c) 2024 Jamjamjon
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.

## NotoSans (SIL Open Font License 1.1) — bundled font

`assets/NotoSans-Regular.ttf` (embedded via `include_bytes!` in `draw.rs`)
is distributed under the SIL Open Font License 1.1 — full text in
`assets/OFL.txt`. The optional CJK override font (`NotoSansSC-Regular.ttf`,
loaded from the extension's `fonts/` dir when present) is under the same
license.

## ONNX Runtime (MIT) — bundled runtime library

The `.nep` packages produced from this repository bundle ONNX Runtime
dynamic libraries (per-platform / per-accelerator variants). Copyright
© Microsoft Corporation. MIT License:
https://github.com/microsoft/onnxruntime/blob/main/LICENSE

## PP-OCR models (Apache-2.0)

PP-OCR detection/recognition ONNX models redistributed per
`ModelSpec` declarations originate from the PaddlePaddle project
(Apache License 2.0).

## ⚠️ Not redistributable commercially without a separate license

- **Ultralytics YOLO weights** (e.g. `yolo11n.onnx`, AGPL-3.0): commercial
  distribution requires an Ultralytics commercial license or replacement
  with an Apache-2.0 detector (YOLOX / RT-DETR / PP-YOLOE). Tracked in
  `extensions/vision-hub/COMMERCIAL.md`.
- **InsightFace pretrained models** (SCRFD/ArcFace): code is MIT, the
  pretrained weights are research-only. The face batch must switch model
  sources before any commercial release.
