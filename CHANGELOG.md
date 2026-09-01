# Changelog

## 2026-08 (unreleased)

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
- **Repo hygiene** — 27 extensions in marketplace index (video-vlm-v2 and
  vision-hub added, names de-corrupted); 6 broken test files fixed;
  video-vlm-v2 fork residue corrected (package id, UMD global, frontend
  manifest); neomind-ext scaffold uses crates.io SDK; docs overhauled.
