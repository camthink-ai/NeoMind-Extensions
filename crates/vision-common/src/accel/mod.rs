//! Hardware acceleration abstraction — the ONLY place execution providers
//! are chosen and ONNX Runtime session constraints are derived.
//!
//! Consolidates what used to be scattered across five extensions:
//! - `auto_device()` compile-time OS gating (image-analyzer / yolo-* ×3)
//! - `nvidia-smi` free-memory probing + RLIMIT_AS management (ocr-device-inference)
//! - `/dev/nvidia0` + `/proc/meminfo` detection (paddle-ocr-v6)
//! - per-EP graph-optimizer constraints that had to be remembered at every
//!   call site (CUDA ⇒ GraphOpt Level 1; GeluFusion family disabled — the
//!   bugs the vendored usls fork used to patch around).
//!
//! Design: [`HardwareProfile::probe()`] does the (impure) detection once;
//! [`plan()`] is a pure function from profile + user hint to a
//! [`DevicePlan`], fully unit-testable without hardware.

pub mod tier;

pub use tier::Tier;

use serde::{Deserialize, Serialize};

/// Optimizers that fuse subgraphs into `com.microsoft.*` contrib ops which
/// CUDA/TRT EPs lack kernels for (EP_FAIL at inference time). Disabled
/// unconditionally — matching the vendored usls fork's behaviour — because
/// the fusions only fire on Gelu-bearing models where they are harmful.
pub const DISABLED_OPTIMIZERS: &str =
    "GeluFusionL1,GeluFusionL2,GeluFusion,BiasGeluFusion,FastGeluFusion";

/// Minimum free GPU memory (MB) for auto-selecting CUDA.
const MIN_GPU_FREE_MB: u64 = 2048;

/// Below this effective RLIMIT_AS (MB), prefer CPU — big-model sessions on
/// GPU need extra virtual address space (ocr-device-inference finding).
const RLIMIT_CPU_FLOOR_MB: u64 = 4096;

/// SoC board kind, probed from `/proc/device-tree/model`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Board {
    Jetson(String),
    Rockchip(String),
    Other(String),
}

impl Board {
    pub fn model(&self) -> &str {
        match self {
            Board::Jetson(m) | Board::Rockchip(m) | Board::Other(m) => m,
        }
    }
}

/// A snapshot of host hardware capability. All fields best-effort.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareProfile {
    pub os: &'static str,
    pub arch: &'static str,
    /// `/dev/nvidia0` present (Linux). Strong evidence of a real driver.
    pub nvidia_device: bool,
    /// Free VRAM in MB from `nvidia-smi`, when available.
    pub gpu_free_mb: Option<u64>,
    pub total_ram_gb: u64,
    /// Effective RLIMIT_AS soft limit in MB after our raise attempt.
    pub rlimit_as_mb: Option<u64>,
    /// Whether we managed to raise RLIMIT_AS soft → hard.
    pub rlimit_raised: bool,
    /// SoC board (device-tree model), Linux only.
    pub board: Option<Board>,
    /// macOS: CoreML is built into the OS on Apple Silicon.
    pub coreml: bool,
}

impl Default for HardwareProfile {
    fn default() -> Self {
        Self {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            nvidia_device: false,
            gpu_free_mb: None,
            total_ram_gb: 16,
            rlimit_as_mb: None,
            rlimit_raised: false,
            board: None,
            coreml: cfg!(target_os = "macos"),
        }
    }
}

impl HardwareProfile {
    /// Probe the host. Impure (reads /proc, runs nvidia-smi/sysctl, may
    /// raise RLIMIT_AS). Cheap enough to call once at startup.
    pub fn probe() -> Self {
        let mut p = Self::default();
        p.nvidia_device = std::path::Path::new("/dev/nvidia0").exists();
        p.gpu_free_mb = gpu_free_memory_mb();
        p.total_ram_gb = total_ram_gb();
        p.board = probe_board();
        #[cfg(all(unix, not(target_arch = "wasm32")))]
        {
            let (soft, raised) = ensure_rlimit_as();
            p.rlimit_as_mb = soft;
            p.rlimit_raised = raised;
        }
        p
    }

    /// Would CUDA plausibly work here (device present + enough free VRAM)?
    pub fn cuda_usable(&self) -> bool {
        self.nvidia_device
            && self.gpu_free_mb.map(|m| m >= MIN_GPU_FREE_MB).unwrap_or(true)
    }
}

/// Which accelerator to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Accelerator {
    Cpu,
    Cuda(u16),
    CoreMl,
    TensorRt(u16),
}

impl Accelerator {
    pub fn as_str(&self) -> String {
        match self {
            Accelerator::Cpu => "cpu".into(),
            Accelerator::Cuda(i) => format!("cuda:{i}"),
            Accelerator::CoreMl => "coreml".into(),
            Accelerator::TensorRt(i) => format!("tensorrt:{i}"),
        }
    }
}

/// User override for device selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DeviceHint {
    #[default]
    Auto,
    Cpu,
    Cuda(u16),
    CoreMl,
    TensorRt(u16),
}

/// The resolved execution plan for one model session. Session builders MUST
/// apply `graph_opt_level` and `disabled_optimizers` — the constraints are
/// data here precisely so call sites cannot forget them (the SVTR models in
/// ocr-device-inference missed the CUDA GraphOpt workaround for exactly
/// this reason).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevicePlan {
    pub accelerator: Accelerator,
    /// `None` = ORT default (Level 3). `Some(1)` on CUDA/TRT: Level 3
    /// triggers MatmulTransposeFusion → `com.microsoft.FusedMatMul` nodes
    /// that CUDA EP lacks kernels for.
    pub graph_opt_level: Option<u8>,
    pub disabled_optimizers: String,
    pub tier: Tier,
    /// Human-readable decision trail, surfaced via `get_status`.
    pub notes: Vec<String>,
}

impl Default for DevicePlan {
    fn default() -> Self {
        Self {
            accelerator: Accelerator::Cpu,
            graph_opt_level: None,
            disabled_optimizers: DISABLED_OPTIMIZERS.to_string(),
            tier: Tier::Tiny,
            notes: Vec::new(),
        }
    }
}

impl DevicePlan {
    pub fn accelerator_str(&self) -> String {
        self.accelerator.as_str()
    }
}

/// Pure decision function: profile + hint → plan.
pub fn plan(profile: &HardwareProfile, hint: &DeviceHint) -> DevicePlan {
    let mut notes = Vec::new();
    let accelerator = match hint {
        DeviceHint::Cpu => {
            notes.push("device: forced cpu by hint".into());
            Accelerator::Cpu
        }
        DeviceHint::Cuda(id) => {
            if profile.cuda_usable() {
                notes.push(format!("device: forced cuda:{id} by hint"));
                Accelerator::Cuda(*id)
            } else {
                notes.push(format!(
                    "device: cuda:{id} hinted but host has no usable GPU, falling back to cpu"
                ));
                Accelerator::Cpu
            }
        }
        DeviceHint::CoreMl => {
            if profile.coreml {
                notes.push("device: forced coreml by hint".into());
                Accelerator::CoreMl
            } else {
                notes.push("device: coreml hinted but not available on this OS, falling back to cpu".into());
                Accelerator::Cpu
            }
        }
        DeviceHint::TensorRt(id) => {
            if profile.cuda_usable() {
                notes.push(format!("device: forced tensorrt:{id} by hint"));
                Accelerator::TensorRt(*id)
            } else {
                notes.push(format!(
                    "device: tensorrt:{id} hinted but host has no usable GPU, falling back to cpu"
                ));
                Accelerator::Cpu
            }
        }
        DeviceHint::Auto => auto_accelerator(profile, &mut notes),
    };

    let mut graph_opt_level = None;
    match accelerator {
        Accelerator::Cuda(_) | Accelerator::TensorRt(_) => {
            graph_opt_level = Some(1);
            notes.push(
                "graph-opt: level 1 (level 3 fuses MatmulTranspose → FusedMatMul, no CUDA kernel)"
                    .into(),
            );
        }
        _ => {}
    }

    let has_cuda = matches!(accelerator, Accelerator::Cuda(_) | Accelerator::TensorRt(_));
    let tier = Tier::Auto.resolve(has_cuda, profile.coreml, profile.total_ram_gb);
    notes.push(format!("tier: {tier} (cuda={has_cuda}, coreml={}, ram={}GB)",
        profile.coreml, profile.total_ram_gb));

    DevicePlan {
        accelerator,
        graph_opt_level,
        disabled_optimizers: DISABLED_OPTIMIZERS.to_string(),
        tier,
        notes,
    }
}

fn auto_accelerator(profile: &HardwareProfile, notes: &mut Vec<String>) -> Accelerator {
    // Memory-limited processes (runner caps): GPU sessions need extra
    // address space; stay on CPU instead of failing mid-session.
    if let Some(rl) = profile.rlimit_as_mb {
        if rl <= RLIMIT_CPU_FLOOR_MB {
            notes.push(format!(
                "device: RLIMIT_AS {rl}MB ≤ {RLIMIT_CPU_FLOOR_MB}MB → cpu"
            ));
            return Accelerator::Cpu;
        }
    }
    if profile.coreml {
        notes.push("device: macOS → coreml".into());
        return Accelerator::CoreMl;
    }
    if profile.cuda_usable() {
        match profile.gpu_free_mb {
            Some(free) => notes.push(format!("device: nvidia gpu with {free}MB free → cuda:0")),
            None => notes.push("device: nvidia device present → cuda:0".into()),
        }
        return Accelerator::Cuda(0);
    }
    if profile.nvidia_device {
        notes.push(format!(
            "device: nvidia device present but free memory {:?} < {MIN_GPU_FREE_MB}MB → cpu",
            profile.gpu_free_mb
        ));
    }
    notes.push("device: no accelerator detected → cpu".into());
    Accelerator::Cpu
}

// ---------------------------------------------------------------------------
// Probing helpers (impure; best-effort, never panic)
// ---------------------------------------------------------------------------

fn gpu_free_memory_mb() -> Option<u64> {
    let out = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=memory.free",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.lines().next()?.trim().parse::<u64>().ok()
}

fn total_ram_gb() -> u64 {
    if let Ok(mem) = std::fs::read_to_string("/proc/meminfo") {
        for line in mem.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                let kb: u64 = rest.trim().trim_end_matches(" kB").trim().parse().unwrap_or(0);
                if kb > 0 {
                    return kb / (1024 * 1024);
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        // x86_64 macs have no /proc — ask sysctl.
        if let Ok(out) = std::process::Command::new("sysctl").args(["-n", "hw.memsize"]).output() {
            if let Ok(bytes) = String::from_utf8_lossy(&out.stdout).trim().parse::<u64>() {
                return bytes / (1024 * 1024 * 1024);
            }
        }
    }
    16
}

fn probe_board() -> Option<Board> {
    let model = std::fs::read_to_string("/proc/device-tree/model").ok()?;
    let model = model.trim_end_matches('\0').trim().to_string();
    if model.is_empty() {
        return None;
    }
    let lower = model.to_lowercase();
    if lower.contains("jetson") || lower.contains("nvidia") {
        Some(Board::Jetson(model))
    } else if lower.contains("rockchip") || lower.starts_with("rk") {
        Some(Board::Rockchip(model))
    } else {
        Some(Board::Other(model))
    }
}

/// Read soft RLIMIT_AS (MB) and try raising soft → hard. Returns
/// (effective soft limit MB, whether a raise happened).
#[cfg(all(unix, not(target_arch = "wasm32")))]
fn ensure_rlimit_as() -> (Option<u64>, bool) {
    unsafe {
        let mut lim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::getrlimit(libc::RLIMIT_AS, &mut lim) != 0 {
            return (None, false);
        }
        let mut raised = false;
        if lim.rlim_max != libc::RLIM_INFINITY && lim.rlim_cur < lim.rlim_max {
            let target = lim.rlim_max;
            if libc::setrlimit(libc::RLIMIT_AS, &libc::rlimit { rlim_cur: target, rlim_max: target }) == 0 {
                lim.rlim_cur = target;
                raised = true;
                tracing::info!(
                    "[accel] raised RLIMIT_AS to {} MB",
                    target / (1024 * 1024)
                );
            }
        }
        let mb = if lim.rlim_cur == libc::RLIM_INFINITY {
            None // unlimited
        } else {
            Some(lim.rlim_cur / (1024 * 1024))
        };
        (mb, raised)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu_profile() -> HardwareProfile {
        HardwareProfile {
            os: "linux",
            arch: "x86_64",
            nvidia_device: false,
            gpu_free_mb: None,
            total_ram_gb: 8,
            rlimit_as_mb: None,
            rlimit_raised: false,
            board: None,
            coreml: false,
        }
    }

    fn mac_profile() -> HardwareProfile {
        HardwareProfile {
            os: "macos",
            coreml: true,
            ..cpu_profile()
        }
    }

    fn cuda_profile(free_mb: u64) -> HardwareProfile {
        HardwareProfile {
            nvidia_device: true,
            gpu_free_mb: Some(free_mb),
            total_ram_gb: 32,
            ..cpu_profile()
        }
    }

    #[test]
    fn auto_cpu_host_gets_cpu_and_tiny_tier() {
        let p = plan(&cpu_profile(), &DeviceHint::Auto);
        assert_eq!(p.accelerator, Accelerator::Cpu);
        assert_eq!(p.graph_opt_level, None);
        assert_eq!(p.tier, Tier::Tiny);
        assert!(!p.notes.is_empty());
    }

    #[test]
    fn auto_macos_gets_coreml_and_small_tier() {
        let p = plan(&mac_profile(), &DeviceHint::Auto);
        assert_eq!(p.accelerator, Accelerator::CoreMl);
        assert_eq!(p.tier, Tier::Small);
    }

    #[test]
    fn auto_cuda_with_vram_gets_cuda_level1_medium() {
        let p = plan(&cuda_profile(8192), &DeviceHint::Auto);
        assert_eq!(p.accelerator, Accelerator::Cuda(0));
        assert_eq!(p.graph_opt_level, Some(1));
        assert_eq!(p.tier, Tier::Medium);
        assert!(p.disabled_optimizers.contains("GeluFusion"));
    }

    #[test]
    fn auto_cuda_low_vram_falls_back_to_cpu() {
        let p = plan(&cuda_profile(512), &DeviceHint::Auto);
        assert_eq!(p.accelerator, Accelerator::Cpu);
        assert!(p.notes.iter().any(|n| n.contains("free memory")));
    }

    #[test]
    fn rlimit_constrained_forces_cpu_even_with_gpu() {
        let mut prof = cuda_profile(8192);
        prof.rlimit_as_mb = Some(2048);
        let p = plan(&prof, &DeviceHint::Auto);
        assert_eq!(p.accelerator, Accelerator::Cpu);
        assert!(p.notes.iter().any(|n| n.contains("RLIMIT_AS")));
    }

    #[test]
    fn rlimit_unlimited_mb_is_none_and_does_not_force_cpu() {
        let mut prof = cpu_profile();
        prof.rlimit_as_mb = None;
        let p = plan(&prof, &DeviceHint::Auto);
        assert_eq!(p.accelerator, Accelerator::Cpu);
    }

    #[test]
    fn hint_cpu_overrides_gpu() {
        let p = plan(&cuda_profile(8192), &DeviceHint::Cpu);
        assert_eq!(p.accelerator, Accelerator::Cpu);
    }

    #[test]
    fn hint_cuda_on_cpu_host_falls_back() {
        let p = plan(&cpu_profile(), &DeviceHint::Cuda(0));
        assert_eq!(p.accelerator, Accelerator::Cpu);
        assert!(p.notes.iter().any(|n| n.contains("no usable GPU")));
    }

    #[test]
    fn hint_coreml_on_linux_falls_back() {
        let p = plan(&cpu_profile(), &DeviceHint::CoreMl);
        assert_eq!(p.accelerator, Accelerator::Cpu);
    }

    #[test]
    fn hint_tensorrt_carries_level1() {
        let p = plan(&cuda_profile(8192), &DeviceHint::TensorRt(0));
        assert_eq!(p.accelerator, Accelerator::TensorRt(0));
        assert_eq!(p.graph_opt_level, Some(1));
    }

    #[test]
    fn board_classification() {
        assert!(matches!(probe_board(), _)); // just runs on this host
    }

    #[test]
    fn cuda_usable_requires_device() {
        let mut p = cpu_profile();
        p.gpu_free_mb = Some(8192); // nvidia-smi present but no /dev/nvidia0
        assert!(!p.cuda_usable());
    }
}
