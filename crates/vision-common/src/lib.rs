//! vision-common: shared vision runtime for NeoMind extensions.
//!
//! This crate is the single home for everything the vision extensions used to
//! copy-paste from each other:
//!
//! - [`accel`] — hardware acceleration abstraction. Probes the host once,
//!   resolves an [`accel::DevicePlan`] (accelerator + session constraints +
//!   model tier), and is the ONLY place that registers ONNX Runtime execution
//!   providers. Direct `ort` integration — no usls.
//! - [`types`] — the unified vision result schema ([`types::Detection`],
//!   [`types::VisionResult`], …) shared by every task.
//! - [`image`] — decode/encode, data-URL handling, letterbox preprocessing,
//!   and metric-value image extraction.
//! - [`native`] — native library bootstrap (ORT_DYLIB_PATH, DYLD/LD paths,
//!   versioned-soname symlinks). Previously duplicated 6×.
//! - [`draw`] — box/label/keypoint overlay rendering with a bundled font.
//! - [`model`] — declarative [`model::ModelSpec`] + [`model::ModelManager`]
//!   (bundle-or-download, checksum, retry, progress).
//! - [`engine`] — direct-ort session building ([`engine::OrtSession`]).
//! - [`models`] — model-family wrappers with their pre/post-processing,
//!   ported from the vendored usls fork (MIT, © Jamjamjon) and from
//!   face-recognition. Detection first ([`models::YoloDetector`]).
//! - [`remote`] — remote inference engines (OpenAI-compatible chat, generic
//!   JSON POST) for board-side / server-side models.
//!
//! Heavy dependencies are feature-gated: pure modules (types/image/draw/model
//! specs) build everywhere including WASM; `engine-ort` pulls in ONNX Runtime.

pub mod accel;
pub mod draw;
pub mod image;
pub mod license;
pub mod model;
pub mod net;
pub mod native;
pub mod remote;
pub mod types;

#[cfg(feature = "engine-ort")]
pub mod engine;
#[cfg(feature = "engine-ort")]
pub mod models;

pub use accel::{DeviceHint, DevicePlan};
pub use types::{BBox, Detection, ImageSource, Keypoint, VisionResult};
