//! Shared ONNX Runtime utility functions.
//!
//! Provides native library path setup and model file discovery shared by
//! the SCRFD detector and ArcFace recognizer modules.

use std::path::PathBuf;

// ============================================================================
// Native Library Path Setup
// ============================================================================

/// Set up native library search paths before ONNX Runtime is loaded.
/// Checks NEOMIND_EXTENSION_DIR/lib/ and common system paths.
#[cfg(not(target_arch = "wasm32"))]
pub fn setup_native_lib_paths() {
    let lib_env = if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    };

    let mut paths = vec![];

    // 1. Extension's bundled libraries
    if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
        let ext_path = std::path::Path::new(&ext_dir);

        let lib_dir = ext_path.join("lib");
        if lib_dir.is_dir() {
            tracing::info!("[OnnxUtils] Adding extension lib dir: {}", lib_dir.display());
            paths.push(lib_dir.to_string_lossy().to_string());
        }

        let binaries_dir = ext_path.join("binaries");
        if binaries_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&binaries_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        tracing::info!("[OnnxUtils] Adding platform dir: {}", path.display());
                        paths.push(path.to_string_lossy().to_string());

                        if let Ok(files) = std::fs::read_dir(&path) {
                            for file in files.flatten() {
                                let file_path = file.path();
                                let name =
                                    file_path.file_name().unwrap_or_default().to_string_lossy();
                                if let Some(base) = name
                                    .strip_suffix(".dylib")
                                    .or_else(|| name.strip_suffix(".so"))
                                {
                                    if base.contains('.') {
                                        let unversioned = if cfg!(target_os = "macos") {
                                            format!(
                                                "{}.dylib",
                                                base.split('.').next().unwrap_or(base)
                                            )
                                        } else {
                                            format!("{}.so", base.split('.').next().unwrap_or(base))
                                        };
                                        let link_path = path.join(&unversioned);
                                        if !link_path.exists() {
                                            #[cfg(unix)]
                                            let _ =
                                                std::os::unix::fs::symlink(&file_path, &link_path);
                                            #[cfg(not(unix))]
                                            let _ = ();
                                            tracing::info!(
                                                "[OnnxUtils] Created symlink: {} -> {}",
                                                unversioned,
                                                name
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let lib_dir = cwd.join("lib");
        if lib_dir.is_dir() {
            paths.push(lib_dir.to_string_lossy().to_string());
        }
    }

    if let Ok(existing) = std::env::var(lib_env) {
        paths.push(existing);
    }

    for dir in ["/opt/homebrew/lib", "/usr/local/lib"] {
        if std::path::Path::new(dir).is_dir() {
            paths.push(dir.to_string());
        }
    }

    if !paths.is_empty() {
        let combined = paths.join(":");
        tracing::info!("[OnnxUtils] Setting {} = {}", lib_env, combined);
        std::env::set_var(lib_env, &combined);
    }

    // Set ORT_DYLIB_PATH to the exact location of libonnxruntime
    if std::env::var("ORT_DYLIB_PATH").is_err() {
        let ort_filename = if cfg!(target_os = "macos") {
            "libonnxruntime.dylib"
        } else if cfg!(target_os = "windows") {
            "onnxruntime.dll"
        } else {
            "libonnxruntime.so"
        };

        for dir in &paths {
            let ort_path = std::path::Path::new(dir).join(ort_filename);
            if ort_path.exists() {
                tracing::info!(
                    "[OnnxUtils] Setting ORT_DYLIB_PATH = {}",
                    ort_path.display()
                );
                std::env::set_var("ORT_DYLIB_PATH", &ort_path);
                break;
            }
        }
    }
}

// ============================================================================
// Model Path Discovery
// ============================================================================

/// Find model file by searching common locations.
///
/// Search order:
/// 1. `NEOMIND_EXTENSION_DIR/models/<filename>` (if env var is set)
/// 2. Current working directory `models/<filename>`
/// 3. Relative fallback paths (`models/`, `../models/`)
pub fn find_model_path(filename: &str) -> Result<PathBuf, String> {
    // If NEOMIND_EXTENSION_DIR is set, use it exclusively
    if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
        let path = PathBuf::from(&ext_dir).join("models").join(filename);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!("Model file '{}' not found in NEOMIND_EXTENSION_DIR/models ({})", filename, ext_dir));
    }

    // Fallback: Check current working directory
    if let Ok(cwd) = std::env::current_dir() {
        let path = cwd.join("models").join(filename);
        if path.exists() {
            return Ok(path);
        }
    }

    // Additional fallback paths
    let fallback_paths = vec![
        PathBuf::from("models").join(filename),
        PathBuf::from("../models").join(filename),
    ];

    for path in fallback_paths {
        if path.exists() {
            return Ok(path);
        }
    }

    Err(format!(
        "Model file '{}' not found in extension models directory",
        filename
    ))
}
