//! Direct-ort session building — replaces the usls `OrtEngine` layer.
//!
//! One job: take a model file + a [`DevicePlan`] and produce a session with
//! the plan's constraints applied (optimization level, disabled optimizers,
//! EP chain). If the plan's accelerator can't actually register (EP missing
//! from the bundled dylib), we log and fall back to CPU rather than fail —
//! the bundled ORT dylib variant decides real capability, code adapts.

use std::path::Path;

use ort::execution_providers::ExecutionProvider as _;

use crate::accel::{Accelerator, DevicePlan};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("ort error: {0}")]
    Ort(String),
    #[error("model file not found: {0}")]
    ModelNotFound(String),
    #[error("bad input: {0}")]
    Input(String),
}

impl From<ort::Error> for EngineError {
    fn from(e: ort::Error) -> Self {
        EngineError::Ort(e.to_string())
    }
}

/// An owned f32 output tensor.
#[derive(Debug, Clone)]
pub struct OutputTensor {
    pub name: String,
    pub shape: Vec<i64>,
    pub data: Vec<f32>,
}

impl OutputTensor {
    /// Row-major index of (…, c, h, w)-style trailing coords.
    /// Out-of-range indices yield 0.0 instead of panicking — decode loops
    /// over potentially malformed tensors must never take the process down.
    pub fn at(&self, idx: &[usize]) -> f32 {
        let mut flat = 0usize;
        for (i, d) in idx.iter().enumerate() {
            let dim = self.shape.get(i).copied().unwrap_or(0).max(0) as usize;
            if *d >= dim {
                return 0.0;
            }
            flat = flat * dim + d;
        }
        self.data.get(flat).copied().unwrap_or(0.0)
    }
}

/// A loaded ONNX session bound to the plan that built it.
pub struct OrtSession {
    session: ort::session::Session,
    pub plan: DevicePlan,
    pub input_name: String,
    pub input_h: u32,
    pub input_w: u32,
    pub input_channels: usize,
}

impl OrtSession {
    /// Build a session for a single-NCHW-f32-input model.
    ///
    /// `input_size`: (channels, height, width) the caller wants to feed.
    /// (Model wrappers own their preprocessing; this records it so callers
    /// and status reporting agree.)
    pub fn from_model(path: &Path, plan: DevicePlan, input_c: usize, input_h: u32, input_w: u32) -> Result<Self, EngineError> {
        if !path.exists() {
            return Err(EngineError::ModelNotFound(path.display().to_string()));
        }
        // Must precede first ORT session creation.
        crate::native::setup_native_lib_paths();

        let mut builder = ort::session::Session::builder()?;

        // Plan constraints — applied unconditionally so call sites can't
        // forget them (the historical SVTR bug).
        if let Some(level) = plan.graph_opt_level {
            let lvl = match level {
                0 => ort::session::builder::GraphOptimizationLevel::Disable,
                1 => ort::session::builder::GraphOptimizationLevel::Level1,
                2 => ort::session::builder::GraphOptimizationLevel::Level2,
                _ => ort::session::builder::GraphOptimizationLevel::Level3,
            };
            builder = builder.with_optimization_level(lvl)?;
        }
        if !plan.disabled_optimizers.is_empty() {
            builder = builder.with_disabled_optimizers(&plan.disabled_optimizers)?;
        }
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        builder = builder
            .with_intra_threads(threads)?
            .with_inter_threads(2)?;

        let mut plan = plan;
        if !register_execution_provider(&mut builder, plan.accelerator)? {
            // Registration failed (EP missing from the bundled dylib) and we
            // fell back to CPU — the plan MUST say so, or accelerator
            // reporting and tier selection become fiction.
            plan.accelerator = crate::accel::Accelerator::Cpu;
            plan.notes.push(format!(
                "engine: {:?} EP unavailable in bundled ORT — running on cpu",
                plan.accelerator
            ));
            plan.tier = crate::accel::Tier::Auto.resolve(false, false, 16);
        }

        let session = builder.commit_from_file(path)?;
        let input_name = session
            .inputs
            .first()
            .map(|i| i.name.to_string())
            .unwrap_or_else(|| "images".to_string());

        Ok(Self {
            session,
            plan,
            input_name,
            input_h,
            input_w,
            input_channels: input_c,
        })
    }

    /// Run one NCHW f32 input, return all f32 outputs owned.
    pub fn run_nchw_f32(&mut self, nchw_data: Vec<f32>) -> Result<Vec<OutputTensor>, EngineError> {
        let shape = vec![
            1i64,
            self.input_channels as i64,
            self.input_h as i64,
            self.input_w as i64,
        ];
        let expected = shape.iter().map(|d| *d as usize).product::<usize>();
        if nchw_data.len() != expected {
            return Err(EngineError::Input(format!(
                "input length {} != {} ({}×{}×{}×{})",
                nchw_data.len(),
                expected,
                1,
                self.input_channels,
                self.input_h,
                self.input_w
            )));
        }
        // Declared output order captured BEFORE run() (its result borrows
        // the session mutably). Callers using outputs.first()/positional
        // access must get deterministic, model-declared ordering — not the
        // HashMap iteration order of the run result.
        let declared: Vec<String> = self
            .session
            .outputs
            .iter()
            .map(|o| o.name.to_string())
            .collect();
        let tensor = ort::value::Tensor::from_array((shape, nchw_data.into_boxed_slice()))?;
        let outputs = self.session.run(ort::inputs![tensor])?;
        let order: Vec<String> = if declared.is_empty() {
            outputs.keys().map(|k| k.to_string()).collect()
        } else {
            declared
        };
        let mut result = Vec::with_capacity(order.len());
        for name in &order {
            let Some(out) = outputs.get(name.as_str()) else {
                continue;
            };
            let (shape, data) = out
                .try_extract_tensor::<f32>()
                .map_err(|e| EngineError::Ort(format!("extract {name}: {e}")))?;
            let dims = (0..shape.len()).map(|i| shape[i] as i64).collect();
            result.push(OutputTensor {
                name: name.clone(),
                shape: dims,
                data: data.to_vec(),
            });
        }
        Ok(result)
    }

    pub fn accelerator_str(&self) -> String {
        self.plan.accelerator_str()
    }
}

/// Register the plan's EP; fall back to CPU when registration fails.
/// CPU needs no registration — ORT always terminates the EP chain with it.
fn register_execution_provider(
    builder: &mut ort::session::builder::SessionBuilder,
    accelerator: Accelerator,
) -> Result<bool, EngineError> {
    let registered = match accelerator {
        Accelerator::Cpu => true,
        Accelerator::Cuda(id) => {
            let ep = ort::execution_providers::CUDAExecutionProvider::default()
                .with_device_id(id as i32);
            match ep.is_available() {
                Ok(true) => ep
                    .register(builder)
                    .map(|_| true)
                    .map_err(|e| {
                        tracing::warn!("[engine] CUDA register failed: {e}");
                        e
                    })
                    .unwrap_or(false),
                _ => {
                    tracing::warn!(
                        "[engine] CUDA unavailable in bundled ORT dylib — using CPU. \
                         Install a CUDA build of onnxruntime (jetson/cuda variant) to accelerate."
                    );
                    false
                }
            }
        }
        Accelerator::CoreMl => {
            let mut ep = ort::execution_providers::CoreMLExecutionProvider::default()
                .with_compute_units(ort::execution_providers::coreml::CoreMLComputeUnits::All);
            if let Ok(dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
                let cache = std::path::PathBuf::from(dir).join("caches").join("coreml");
                match std::fs::create_dir_all(&cache) {
                    Ok(()) => {
                        ep = ep.with_model_cache_dir(cache.display().to_string());
                    }
                    Err(e) => {
                        // Without a cache dir CoreML re-specializes the model
                        // on every process start (multi-second first run).
                        tracing::warn!(
                            "[engine] CoreML cache dir unavailable ({e}) —                              expect slower first inference"
                        );
                    }
                }
            }
            match ep.is_available() {
                Ok(true) => ep
                    .register(builder)
                    .map(|_| true)
                    .map_err(|e| {
                        tracing::warn!("[engine] CoreML register failed: {e}");
                        e
                    })
                    .unwrap_or(false),
                _ => {
                    tracing::warn!("[engine] CoreML unavailable — using CPU");
                    false
                }
            }
        }
        Accelerator::TensorRt(id) => {
            #[cfg(not(feature = "tensorrt"))]
            {
                let _ = id;
                tracing::warn!(
                    "[engine] TensorRT hinted but vision-common built without `tensorrt` feature — using CPU"
                );
                false
            }
            #[cfg(feature = "tensorrt")]
            {
                let ep = ort::execution_providers::TensorRTExecutionProvider::default()
                    .with_device_id(id as i32);
                match ep.is_available() {
                    Ok(true) => ep
                        .register(builder)
                        .map(|_| true)
                        .map_err(|e| {
                            tracing::warn!("[engine] TensorRT register failed: {e}");
                            e
                        })
                        .unwrap_or(false),
                    _ => {
                        tracing::warn!("[engine] TensorRT unavailable in bundled dylib — using CPU");
                        false
                    }
                }
            }
        }
    };
    if !registered {
        tracing::info!("[engine] falling back to CPU execution provider");
    }
    Ok(registered)
}
