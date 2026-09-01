//! Model-family wrappers with pre/post-processing, ported from the vendored
//! usls fork (MIT, © 2024 Jamjamjon — see README) and from face-recognition.
//! Detection layouts only for now; pose/segmentation land with their tasks.

pub mod yolo;

pub use yolo::{decode_detections, find_model, YoloDetector, YoloLayout};
