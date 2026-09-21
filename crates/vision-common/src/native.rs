//! Native library bootstrap — the consolidated copy of `setup_native_lib_paths()`
//! that used to exist verbatim in six extensions.
//!
//! Must run BEFORE the first ONNX Runtime session is created: discovers the
//! bundled ORT dylib under `NEOMIND_EXTENSION_DIR`, injects DYLD/LD_LIBRARY_PATH,
//! creates unversioned soname symlinks, and points `ORT_DYLIB_PATH` at the
//! exact file (load-dynamic's recommended mechanism — runtime DYLD edits may
//! not affect dlopen on macOS).

use std::path::{Path, PathBuf};
use std::sync::Once;

static INIT: Once = Once::new();

/// Idempotent: the first call sets everything up, later calls are no-ops.
///
/// THREAD-SAFETY: `std::env::set_var` racing concurrent `var` reads on
/// other threads is formally UB (why Rust 2024 makes it unsafe). The
/// platform's env_hints mechanism sets ORT_DYLIB_PATH BEFORE the process
/// starts — when it's already present we skip ALL env mutation, confining
/// the (legacy/dev-only) mutation window to single-threaded startup.
pub fn setup_native_lib_paths() {
    INIT.call_once(|| {
        if std::env::var_os("ORT_DYLIB_PATH").is_some() {
            // Platform-injected (env_hints) or operator-provided — nothing
            // to bootstrap, and mutating DYLD/LD at runtime is unreliable
            // on macOS anyway.
            return;
        }
        let paths = discover_paths();
        apply_env(&paths);
    });
}

/// Locate candidate library directories (extension bundle first, then cwd,
/// then common system paths).
fn discover_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
        let ext_path = Path::new(&ext_dir);

        let lib_dir = ext_path.join("lib");
        if lib_dir.is_dir() {
            tracing::info!("[native] lib dir: {}", lib_dir.display());
            paths.push(lib_dir);
        }

        // binaries/<platform>/ holds extension.dylib plus every dylib the
        // packager bundled next to it.
        let binaries_dir = ext_path.join("binaries");
        if binaries_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&binaries_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        tracing::info!("[native] platform dir: {}", path.display());
                        paths.push(path.clone());
                        symlink_unversioned_libs(&path);
                    }
                }
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let lib_dir = cwd.join("lib");
        if lib_dir.is_dir() {
            paths.push(lib_dir);
        }
    }

    for dir in ["/opt/homebrew/lib", "/usr/local/lib"] {
        if Path::new(dir).is_dir() {
            paths.push(PathBuf::from(dir));
        }
    }

    paths
}

/// Create unversioned symlinks for versioned shared libraries:
/// `libonnxruntime.so.1` → `libonnxruntime.so`,
/// `libonnxruntime.1.22.0.dylib` → `libonnxruntime.dylib`.
fn symlink_unversioned_libs(dir: &Path) {
    let Ok(files) = std::fs::read_dir(dir) else {
        return;
    };
    for file in files.flatten() {
        let file_path = file.path();
        let name = file_path.file_name().unwrap_or_default().to_string_lossy().to_string();

        let unversioned = if cfg!(target_os = "macos") {
            // Only treat as a versioned SHARED LIBRARY (libfoo.1.2.3.dylib)
            // when the first dot-segment looks like one — "my.lib.1.dylib"
            // must not gain a bogus "my.dylib" link.
            name.strip_suffix(".dylib").and_then(|base| {
                let parts: Vec<&str> = base.split('.').collect();
                if parts.len() > 1 && parts[0].starts_with("lib") {
                    Some(format!("{}.dylib", parts[0]))
                } else {
                    None
                }
            })
        } else if cfg!(target_os = "windows") {
            None
        } else {
            name.find(".so.").map(|idx| format!("{}.so", &name[..idx]))
        };

        if let Some(unversioned) = unversioned {
            let link_path = dir.join(&unversioned);
            if !link_path.exists() {
                #[cfg(unix)]
                let _ = std::os::unix::fs::symlink(&file_path, &link_path);
                tracing::info!("[native] symlink: {} -> {}", unversioned, name);
            }
        }
    }
}

fn apply_env(paths: &[PathBuf]) {
    let lib_env = if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else if cfg!(target_os = "windows") {
        "PATH"
    } else {
        "LD_LIBRARY_PATH"
    };

    let mut entries: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
    if let Ok(existing) = std::env::var(lib_env) {
        entries.push(existing);
    }
    if !entries.is_empty() {
        let sep = if cfg!(target_os = "windows") { ";" } else { ":" };
        let combined = entries.join(sep);
        tracing::info!("[native] {} = {}", lib_env, combined);
        std::env::set_var(lib_env, &combined);
    }

    // Point ORT_DYLIB_PATH at the exact dylib — set_var of DYLD_* may not
    // influence dlopen on macOS, but load-dynamic reads this directly.
    if std::env::var("ORT_DYLIB_PATH").is_err() {
        let ort_filename = if cfg!(target_os = "macos") {
            "libonnxruntime.dylib"
        } else if cfg!(target_os = "windows") {
            "onnxruntime.dll"
        } else {
            "libonnxruntime.so"
        };
        for dir in paths {
            let ort_path = dir.join(ort_filename);
            if ort_path.exists() {
                tracing::info!("[native] ORT_DYLIB_PATH = {}", ort_path.display());
                std::env::set_var("ORT_DYLIB_PATH", &ort_path);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_idempotently() {
        setup_native_lib_paths();
        setup_native_lib_paths();
    }

    #[test]
    fn discovers_system_dirs_on_this_host() {
        let paths = discover_paths();
        // /opt/homebrew/lib exists on this dev machine (arm64 mac)
        if cfg!(target_os = "macos") && Path::new("/opt/homebrew/lib").exists() {
            assert!(paths.iter().any(|p| p.ends_with("/opt/homebrew/lib")));
        }
    }
}
