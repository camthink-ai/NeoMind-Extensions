# Changelog

## 2026-08 (unreleased)

- **gym-tracker 2.10.0** — binary push frames end-to-end (rides the platform's
  new opt-in binary channel, NeoMind core ≥ 0.9.23): ingest decodes the
  device's `img_b64` once and stores `Arc<Vec<u8>>`; the push thread emits an
  `application/x-neomind-frame` container (`[u32 meta_len][tracks/faces/ts
  JSON][JPEG]`), eliminating BOTH base64 layers on the browser leg (~-43%
  wire for the Monitor, no per-frame `atob`/130 KB JSON parse). Monitor
  frontend: binary-frame branch + `createImageBitmap` (async off-main-thread
  decode, jitter buffer holds encoded bytes and decodes at draw time),
  legacy Text/REST paths retained for old servers; `vel` added to the `Track`
  TS interface (was `as any`), dead `start_push` WS message removed,
  frame dedup switched to device `ts_ns`. REST `get_frame` still returns an
  `img_b64` string for old frontends.
- **vision-hub 0.1.0** — unified vision extension (detect batch): pipeline
  engine over device frames, hardware-accelerated inference via
  `crates/vision-common` (direct ort, no usls), virtual-metric/event sinks,
  capture rules, offline license gating. See `extensions/vision-hub/`.
- **crates/vision-common 0.1.0** — shared vision runtime: accel
  abstraction (HardwareProfile/DevicePlan/Tier), direct-ort engine, YOLO
  decoders (v5/v8/v10 port from usls, MIT), image IO, drawing, model
  manager (download/verify), remote engines, license verification.
- **SDK 0.6.5** — typed events mirror (`events`), multimodal
  `chat::invoke_with_images`, manifest `env_hints` field; ABI unchanged (3).
- **Platform data-dir unification** (NeoMind core) — all redb stores honor
  `NEOMIND_DATA_DIR` with legacy fallback; extension-private
  `NEOMIND_EXTENSION_DATA_DIR` survives upgrades and uninstall.
- **Build/release** — update-versions.sh is the authoritative JSON
  generator (BSD-sed name corruption fixed; env_hints + variant entries
  preserved); build.sh dev-install now copies models; ORT packaging no
  longer duplicates mismatched versioned dylibs.
- **Repo hygiene** — 27 extensions in marketplace index (video-vlm and
  vision-hub added, names de-corrupted); 6 broken test files fixed;
  video-vlm fork residue corrected (package id, UMD global, frontend
  manifest); neomind-ext scaffold uses crates.io SDK; docs overhauled.
