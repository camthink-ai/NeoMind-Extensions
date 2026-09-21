//! Inference engine layer. [`ort_session`] is the direct-ort implementation
//! that applies a [`crate::accel::DevicePlan`] — the single place execution
//! providers get registered.

pub mod ort_session;

pub use ort_session::{OrtSession, OutputTensor};
