//! Declarative model specs + a bundle-or-download manager — the generalized
//! version of paddle-ocr-v6's downloader (the only extension that had one).
//!
//! Small models ship inside the .nep (`models/` dir → `NEOMIND_EXTENSION_DIR/models`);
//! big models declare a URL and are fetched lazily on first pipeline start,
//! with `.part` staging, retry + exponential backoff, content-length and
//! sha256 verification, and progress reporting for metrics.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// One downloadable model artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    /// Canonical filename inside the cache dir (e.g. `yolo11n.onnx`).
    pub filename: String,
    /// Source URL; `None` means bundled in the .nep, never downloaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// sha256 hex (lowercase) for integrity; enforced when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Expected size in bytes, enforced when present (also used for
    /// download progress even without a callback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

impl ModelSpec {
    pub fn bundled(filename: impl Into<String>) -> Self {
        Self {
            filename: filename.into(),
            url: None,
            sha256: None,
            size: None,
        }
    }

    pub fn remote(filename: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            filename: filename.into(),
            url: Some(url.into()),
            sha256: None,
            size: None,
        }
    }

    pub fn with_sha256(mut self, sha: impl Into<String>) -> Self {
        self.sha256 = Some(sha.into());
        self
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("io: {0}")]
    Io(String),
    #[error("http: {0}")]
    Http(String),
    #[error("integrity: {0}")]
    Integrity(String),
    #[error("not found: {0}")]
    NotFound(String),
}

/// Per-mille progress 0..=1000 for the current ensure() run.
pub type Progress = std::sync::Arc<AtomicU64>;

pub struct ModelManager {
    cache_dir: PathBuf,
    pub progress: Progress,
}

impl ModelManager {
    /// Cache dir defaults to `$NEOMIND_EXTENSION_DIR/models`, else `./models`.
    pub fn new() -> Self {
        let cache_dir = std::env::var("NEOMIND_EXTENSION_DIR")
            .map(|d| PathBuf::from(d).join("models"))
            .unwrap_or_else(|_| PathBuf::from("models"));
        Self::with_cache_dir(cache_dir)
    }

    pub fn with_cache_dir(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            progress: std::sync::Arc::new(AtomicU64::new(1000)),
        }
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn local_path(&self, spec: &ModelSpec) -> PathBuf {
        self.cache_dir.join(&spec.filename)
    }

    /// Is the spec's artifact present (and checksum-valid if declared)?
    pub fn is_ready(&self, spec: &ModelSpec) -> bool {
        let path = self.local_path(spec);
        if !path.exists() {
            return false;
        }
        self.verify(&path, spec).is_ok()
    }

    /// Ensure present; download when missing. Idempotent.
    pub fn ensure(&self, spec: &ModelSpec) -> Result<PathBuf, ModelError> {
        if let Some(url) = &spec.url {
            if !self.is_ready(spec) {
                self.progress.store(0, Ordering::SeqCst);
                let target = self.local_path(spec);
                self.download_with_retry(url, &target, spec)?;
            }
        }
        self.progress.store(1000, Ordering::SeqCst);
        let path = self.local_path(spec);
        if path.exists() {
            Ok(path)
        } else {
            Err(ModelError::NotFound(format!(
                "{} is not bundled and has no download URL",
                path.display()
            )))
        }
    }

    fn verify(&self, path: &Path, spec: &ModelSpec) -> Result<(), ModelError> {
        if let Some(expected) = spec.size {
            let actual = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            if actual != expected {
                return Err(ModelError::Integrity(format!(
                    "{}: size {} != expected {}",
                    path.display(),
                    actual,
                    expected
                )));
            }
        }
        if let Some(expected) = &spec.sha256 {
            let bytes = std::fs::read(path).map_err(|e| ModelError::Io(e.to_string()))?;
            let mut h = Sha256::new();
            h.update(&bytes);
            let got = hex(&h.finalize());
            if got != expected.to_lowercase() {
                return Err(ModelError::Integrity(format!(
                    "{}: sha256 mismatch (got {got})",
                    path.display()
                )));
            }
        }
        Ok(())
    }

    fn download_with_retry(&self, url: &str, target: &Path, spec: &ModelSpec) -> Result<(), ModelError> {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ModelError::Io(e.to_string()))?;
        }
        let tmp = target.with_extension("part");
        let retries = 3u32;
        let mut last_err = None;
        for attempt in 0..retries {
            match self.download_once(url, &tmp, spec) {
                Ok(()) => {
                    if let Err(e) = self.verify(&tmp, spec) {
                        let _ = std::fs::remove_file(&tmp);
                        return Err(e);
                    }
                    std::fs::rename(&tmp, target).map_err(|e| {
                        ModelError::Io(format!("rename {} -> {}: {e}", tmp.display(), target.display()))
                    })?;
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!("download {}/{} failed for {url}: {e}", attempt + 1, retries);
                    last_err = Some(e);
                    let _ = std::fs::remove_file(&tmp);
                    std::thread::sleep(Duration::from_secs(1u64 << attempt));
                }
            }
        }
        Err(ModelError::Http(format!(
            "download failed after {retries} attempts: {url} ({})",
            last_err.map(|e| e.to_string()).unwrap_or_else(|| "unknown".into())
        )))
    }

    fn download_once(&self, url: &str, target: &Path, spec: &ModelSpec) -> Result<(), ModelError> {
        let resp = ureq::get(url)
            .timeout(Duration::from_secs(300))
            .call()
            .map_err(|e| ModelError::Http(e.to_string()))?;
        let status = resp.status();
        if status >= 400 {
            return Err(ModelError::Http(format!("HTTP {status} for {url}")));
        }
        // spec.size (declared at build time) is AUTHORITATIVE when present:
        // a lying/malicious server's Content-Length must not defeat the
        // truncation check. Without a declared size, cap the download so a
        // hostile source cannot fill the disk.
        const MAX_UNDECLARED_DOWNLOAD: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB
        let expected_len = spec
            .size
            .or_else(|| resp.header("Content-Length").and_then(|s| s.parse::<u64>().ok()))
            .or(Some(MAX_UNDECLARED_DOWNLOAD));

        let mut reader = resp.into_reader();
        let mut file = std::fs::File::create(target).map_err(|e| ModelError::Io(e.to_string()))?;
        let mut written = 0u64;
        let mut buf = [0u8; 64 * 1024];
        loop {
            use std::io::Read;
            let n = reader.read(&mut buf).map_err(|e| ModelError::Io(e.to_string()))?;
            if n == 0 {
                break;
            }
            use std::io::Write;
            file.write_all(&buf[..n]).map_err(|e| ModelError::Io(e.to_string()))?;
            written += n as u64;
            if let Some(total) = expected_len {
                if total > 0 {
                    let pm = (written.min(total) * 1000) / total;
                    self.progress.store(pm, Ordering::SeqCst);
                }
            }
        }
        drop(file);
        if let Some(expected) = expected_len {
            if written != expected {
                return Err(ModelError::Http(format!(
                    "truncated download of {url}: {written} / {expected} bytes"
                )));
            }
        }
        Ok(())
    }

    /// Inventory of cached artifacts (filename, size_bytes) for `get_status`.
    pub fn inventory(&self) -> Vec<(String, u64)> {
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.cache_dir) {
            for e in entries.flatten() {
                let path = e.path();
                if path.is_file() {
                    if let Some(name) = path.file_name() {
                        if let Ok(meta) = std::fs::metadata(&path) {
                            out.push((name.to_string_lossy().to_string(), meta.len()));
                        }
                    }
                }
            }
        }
        out.sort();
        out
    }
}

impl Default for ModelManager {
    fn default() -> Self {
        Self::new()
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vision-common-model-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ensure_bundled_needs_no_url() {
        let dir = tmpdir("bundled");
        std::fs::write(dir.join("tiny.onnx"), b"weights").unwrap();
        let mgr = ModelManager::with_cache_dir(dir);
        let spec = ModelSpec::bundled("tiny.onnx");
        let path = mgr.ensure(&spec).unwrap();
        assert!(path.exists());
        assert!(mgr.is_ready(&spec));
    }

    #[test]
    fn ensure_bundled_missing_errors_without_download() {
        let dir = tmpdir("missing");
        let mgr = ModelManager::with_cache_dir(dir.clone());
        let spec = ModelSpec::bundled("ghost.onnx");
        let err = mgr.ensure(&spec).unwrap_err();
        assert!(err.to_string().contains("not bundled"));
    }

    #[test]
    fn sha256_mismatch_detected() {
        let dir = tmpdir("sha");
        std::fs::write(dir.join("m.onnx"), b"corrupted").unwrap();
        let mgr = ModelManager::with_cache_dir(dir);
        let spec = ModelSpec::bundled("m.onnx")
            .with_sha256("0000000000000000000000000000000000000000000000000000000000000000");
        assert!(!mgr.is_ready(&spec));
    }

    #[test]
    fn sha256_match_passes() {
        let dir = tmpdir("sha-ok");
        std::fs::write(dir.join("ok.onnx"), b"weights").unwrap();
        // compute the digest of the exact bytes we wrote
        let mut h = Sha256::new();
        h.update(b"weights");
        let digest = hex(&h.finalize());
        let mgr = ModelManager::with_cache_dir(dir);
        let spec = ModelSpec::bundled("ok.onnx").with_sha256(digest);
        assert!(mgr.is_ready(&spec));
    }

    #[test]
    fn size_mismatch_detected() {
        let dir = tmpdir("size");
        std::fs::write(dir.join("s.onnx"), b"12345").unwrap();
        let mgr = ModelManager::with_cache_dir(dir);
        let spec = ModelSpec::bundled("s.onnx").with_size(99);
        assert!(!mgr.is_ready(&spec));
    }

    #[test]
    fn inventory_lists_files() {
        let dir = tmpdir("inv");
        std::fs::write(dir.join("a.onnx"), b"aaaa").unwrap();
        std::fs::write(dir.join("b.txt"), b"bb").unwrap();
        let mgr = ModelManager::with_cache_dir(dir);
        let inv = mgr.inventory();
        assert_eq!(inv.len(), 2);
        assert_eq!(inv[0], ("a.onnx".to_string(), 4));
    }

    #[test]
    fn spec_serde_roundtrip() {
        let spec = ModelSpec::remote("yolo11n.onnx", "https://example.com/yolo11n.onnx")
            .with_sha256("abc")
            .with_size(10);
        let j = serde_json::to_string(&spec).unwrap();
        let back: ModelSpec = serde_json::from_str(&j).unwrap();
        assert_eq!(back.filename, spec.filename);
        assert_eq!(back.url, spec.url);
        assert_eq!(back.size, Some(10));
        // bundled spec serializes compactly
        let j2 = serde_json::to_string(&ModelSpec::bundled("x.onnx")).unwrap();
        assert!(!j2.contains("url"));
        assert!(!j2.contains("sha256"));
    }
}
