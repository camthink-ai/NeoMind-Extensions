//! Model-artifact downloader for the laya-decision extension.
//!
//! The .nep ships the ENGINE only; the checkpoint set (~167MB int8 / ~1.1GB
//! fp32) is a separate asset. This module fetches it from a configurable
//! base URL (default: the NeoMind-Extensions GitHub release) with the
//! downloader contract proven by paddle-ocr-v6: 3 retries, temp-file atomic
//! replace, Content-Length verification — plus a `.sha256` sidecar check
//! when the server provides one (the half-written-GGUF lesson: size alone
//! can pass on a truncated CDN write).
//!
//! Progress is published as per-mille on an AtomicU64 the metrics producer
//! and the `status` command read; a failed run leaves existing artifacts
//! untouched (each file lands only after its checksum passes).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use neomind_extension_sdk::{ExtensionError, Result};

/// Default artifact base (v22 int8 tier). Release assets live next to the
/// .nep bundles; override with the `modelBase` config key.
pub const DEFAULT_MODEL_BASE: &str =
    "https://github.com/camthink-ai/NeoMind-Extensions/releases/download/decision-v22";

/// Files of the int8 tier, in download order (small files first so a fresh
/// install reaches a verifiable state quickly).
pub const ARTIFACT_FILES: &[&str] = &[
    "rl_agent_config.json",
    "remap.json",
    "golden.jsonl",
    "tokenizer/tokenizer.json",
    "tokenizer/tokenizer_config.json",
    "model_int8.onnx",
];

pub struct Downloader {
    /// Per-mille progress 0..=1000 across ALL files of a run.
    pub progress: Arc<AtomicU64>,
    /// 1 while a download thread is running (status/UI hint).
    pub active: Arc<AtomicU64>,
}

impl Default for Downloader {
    fn default() -> Self {
        Self {
            progress: Arc::new(AtomicU64::new(1000)),
            active: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl Downloader {
    pub fn new() -> Self {
        Self::default()
    }

    /// True when every artifact file already exists with size > 0.
    pub fn artifacts_present(models_dir: &Path) -> bool {
        ARTIFACT_FILES.iter().all(|f| {
            std::fs::metadata(models_dir.join(f))
                .map(|m| m.len() > 0)
                .unwrap_or(false)
        })
    }

    /// Missing files of the int8 tier, as (url, target) pairs.
    pub fn missing(base: &str, models_dir: &Path) -> Vec<(String, PathBuf)> {
        ARTIFACT_FILES
            .iter()
            .filter(|f| {
                std::fs::metadata(models_dir.join(**f))
                    .map(|m| m.len() > 0)
                    .unwrap_or(false)
                    == false
            })
            .map(|f| (format!("{base}/{f}"), models_dir.join(f)))
            .collect()
    }

    /// Fetch every missing file (retrying each up to 3 times). Existing
    /// files are never re-downloaded. Spawned on a background thread by the
    /// `download` command; this method blocks.
    pub fn ensure_models(&self, base: &str, models_dir: &Path) -> Result<()> {
        let missing = Self::missing(base, models_dir);
        if missing.is_empty() {
            self.progress.store(1000, Ordering::SeqCst);
            return Ok(());
        }
        let total = missing.len() as u64;
        for (i, (url, target)) in missing.iter().enumerate() {
            self.download_with_retry(url, target, 3)?;
            let pct = ((i + 1) as f64 / total as f64 * 1000.0) as u64;
            self.progress.store(pct, Ordering::SeqCst);
        }
        self.progress.store(1000, Ordering::SeqCst);
        Ok(())
    }

    fn download_with_retry(&self, url: &str, target: &Path, retries: u32) -> Result<()> {
        let tmp = target.with_extension("part");
        let mut last = String::new();
        for attempt in 1..=retries {
            match self.download_once(url, &tmp) {
                Ok(()) => {
                    std::fs::rename(&tmp, target).map_err(|e| {
                        ExtensionError::ExecutionFailed(format!(
                            "rename {} -> {} failed: {}",
                            tmp.display(),
                            target.display(),
                            e
                        ))
                    })?;
                    return Ok(());
                }
                Err(e) => {
                    last = format!("{e}");
                    let _ = std::fs::remove_file(&tmp);
                    if attempt < retries {
                        std::thread::sleep(Duration::from_secs(3u64 * attempt as u64));
                    }
                }
            }
        }
        Err(ExtensionError::ExecutionFailed(format!(
            "download {} failed after {retries} attempts: {last}",
            url
        )))
    }

    fn download_once(&self, url: &str, tmp: &Path) -> Result<()> {
        let resp = ureq::get(url)
            .timeout(Duration::from_secs(600))
            .call()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("HTTP: {}", e)))?;
        let status = resp.status();
        if status >= 400 {
            return Err(ExtensionError::ExecutionFailed(format!(
                "HTTP {status} for {url}"
            )));
        }
        let expected_len = resp
            .header("Content-Length")
            .and_then(|s| s.parse::<u64>().ok());
        if let Some(parent) = tmp.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ExtensionError::ExecutionFailed(format!("mkdir {}: {e}", parent.display()))
            })?;
        }
        let mut reader = resp.into_reader();
        let mut file = std::fs::File::create(tmp)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("create {}: {e}", tmp.display())))?;
        std::io::copy(&mut reader, &mut file).map_err(|e| {
            ExtensionError::ExecutionFailed(format!("write {}: {e}", tmp.display()))
        })?;
        drop(file);
        if let Some(expected) = expected_len {
            let actual = std::fs::metadata(tmp).map(|m| m.len()).unwrap_or(0);
            if actual != expected {
                return Err(ExtensionError::ExecutionFailed(format!(
                    "size mismatch for {url}: expected {expected}, got {actual}"
                )));
            }
        }
        // Optional .sha256 sidecar: verify when the server provides one.
        if let Some(expected_hex) = fetch_sha256(url) {
            let actual = sha256_file(tmp)?;
            if actual != expected_hex {
                return Err(ExtensionError::ExecutionFailed(format!(
                    "checksum mismatch for {url}: expected {expected_hex}, got {actual}"
                )));
            }
        }
        Ok(())
    }
}

fn fetch_sha256(url: &str) -> Option<String> {
    let resp = ureq::get(&format!("{url}.sha256"))
        .timeout(Duration::from_secs(60))
        .call()
        .ok()?;
    if resp.status() >= 400 {
        return None;
    }
    let mut body = String::new();
    use std::io::Read;
    resp.into_reader()
        .take(128)
        .read_to_string(&mut body)
        .ok()?;
    let hex = body.split_whitespace().next()?.to_ascii_lowercase();
    (hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit())).then_some(hex)
}

fn sha256_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let data = std::fs::read(path)
        .map_err(|e| ExtensionError::ExecutionFailed(format!("read {}: {e}", path.display())))?;
    let mut h = Sha256::new();
    h.update(&data);
    Ok(format!("{:x}", h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_detects_absent_files() {
        let dir = std::env::temp_dir().join("laya-dl-test-missing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let m = Downloader::missing("http://example.test", &dir);
        assert_eq!(m.len(), ARTIFACT_FILES.len());
        assert!(m.iter().all(|(u, _)| u.starts_with("http://example.test/")));
    }

    #[test]
    fn present_when_all_files_exist() {
        let dir = std::env::temp_dir().join("laya-dl-test-present");
        let _ = std::fs::remove_dir_all(&dir);
        for f in ARTIFACT_FILES {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
        assert!(Downloader::artifacts_present(&dir));
    }
}
