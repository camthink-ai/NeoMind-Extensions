//! Spec §6.3 — Pre-flight checks.
//!
//! Verifies DeepStream package, required GStreamer plugins, and pyds importability
//! before the extension attempts to spawn the sidecar. Results are surfaced via
//! `SystemStatus` for use in error messages and the `system_status` command.

use std::io;
use std::process::Stdio;

use async_trait::async_trait;

/// Required GStreamer plugins (spec §6.3).
const REQUIRED_GST_PLUGINS: &[&str] = &["nvinfer", "nvtracker", "nvdsanalytics", "nvrtspoutsink"];

/// Python interpreters to try, in order (spec §6.3).
const PYTHON_CANDIDATES: &[&str] = &["python3.10", "python3"];

/// Abstraction over shell-style command execution so the pre-flight checks are
/// unit-testable without spawning real processes.
#[async_trait]
pub trait CommandRunner: Send + Sync {
    /// Run a command. Returns `(exit_success, stdout, stderr)`.
    async fn run(&self, program: &str, args: &[&str]) -> io::Result<(bool, String, String)>;
}

/// Production runner backed by `tokio::process::Command`.
pub struct TokioRunner;

#[async_trait]
impl CommandRunner for TokioRunner {
    async fn run(&self, program: &str, args: &[&str]) -> io::Result<(bool, String, String)> {
        let output = tokio::process::Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        Ok((output.status.success(), stdout, stderr))
    }
}

/// Result of running all pre-flight checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemStatus {
    pub deepstream_installed: bool,
    /// Version string parsed from dpkg column 3 (e.g. `7.1.0-1`).
    pub deepstream_version: Option<String>,
    pub pyds_available: bool,
    /// `pyds.__version__` string, if reported.
    pub pyds_version: Option<String>,
    pub gst_plugins_ok: bool,
    /// Names of required plugins that `gst-inspect-1.0` could not find.
    pub gst_missing: Vec<String>,
    /// `"python3.10"` or `"python3"` — whichever first imported pyds.
    pub python_bin: Option<String>,
    /// Epoch millis when the checks were last run.
    pub last_check_at: i64,
    /// Copy-paste install hints, populated iff `!all_ok()`. Empty otherwise.
    pub install_hint: String,
}

impl SystemStatus {
    /// Run all checks with the production `TokioRunner`.
    pub async fn run_checks() -> Self {
        Self::run_checks_with(&TokioRunner).await
    }

    /// Run all checks with an injected runner (for tests).
    ///
    /// Order per spec §6.3 implementation guidance: dpkg → gst → pyds.
    pub async fn run_checks_with(runner: &dyn CommandRunner) -> Self {
        let (deepstream_installed, deepstream_version) = check_deepstream_package(runner).await;
        let (gst_plugins_ok, gst_missing) = check_gst_plugins(runner).await;
        let (pyds_available, pyds_version, python_bin) = check_pyds(runner).await;

        let mut status = Self {
            deepstream_installed,
            deepstream_version,
            pyds_available,
            pyds_version,
            gst_plugins_ok,
            gst_missing,
            python_bin,
            last_check_at: chrono::Utc::now().timestamp_millis(),
            install_hint: String::new(),
        };

        if !status.all_ok() {
            status.install_hint = build_install_hint(&status);
        }

        status
    }

    /// True iff every check passed.
    pub fn all_ok(&self) -> bool {
        self.deepstream_installed && self.pyds_available && self.gst_plugins_ok
    }
}

/// Spec §6.3 #1: `dpkg -l | grep -E '^ii.*deepstream-7'`.
///
/// This is a pipeline so it goes through `sh -c`. The grep is considered
/// successful (exit 0) only when at least one matching line exists.
async fn check_deepstream_package(
    runner: &dyn CommandRunner,
) -> (bool, Option<String>) {
    // sh -c "dpkg -l | grep -E '^ii.*deepstream-7'"
    let pipeline = "dpkg -l | grep -E '^ii.*deepstream-7'";
    match runner.run("sh", &["-c", pipeline]).await {
        Ok((true, stdout, _)) => {
            // grep exit 0 → DeepStream package is installed. Best-effort
            // version extraction; if the row shape is unexpected we still
            // report installed=true (spec: "be lenient").
            let version = stdout.lines().find_map(parse_dpkg_line);
            (true, version)
        }
        _ => (false, None),
    }
}

/// Parse a dpkg `ii` line. Returns column 3 (version) if the row is well-formed.
///
/// Example: `ii  deepstream-7.1   7.1.0-1   amd64   NVIDIA DeepStream SDK`
/// → `Some("7.1.0-1")`.
fn parse_dpkg_line(line: &str) -> Option<String> {
    let cols: Vec<&str> = line.split_whitespace().collect();
    // Columns: 0=state, 1=package, 2=version, 3=arch, 4..=description.
    if cols.len() >= 3 {
        Some(cols[2].to_string())
    } else {
        None
    }
}

/// Spec §6.3 #2: run `gst-inspect-1.0 <plugin>` for each required plugin.
async fn check_gst_plugins(runner: &dyn CommandRunner) -> (bool, Vec<String>) {
    let mut missing = Vec::new();
    for plugin in REQUIRED_GST_PLUGINS {
        let ok = match runner.run("gst-inspect-1.0", &[plugin]).await {
            Ok((success, _, _)) => success,
            Err(_) => false,
        };
        if !ok {
            missing.push((*plugin).to_string());
        }
    }
    (missing.is_empty(), missing)
}

/// Spec §6.3 #3: try `python3.10 -c 'import pyds; print(pyds.__version__)'`,
/// fall back to `python3`. The interpreter that succeeds is stored for reuse.
async fn check_pyds(
    runner: &dyn CommandRunner,
) -> (bool, Option<String>, Option<String>) {
    let code = "import pyds; print(pyds.__version__)";
    for candidate in PYTHON_CANDIDATES {
        match runner.run(candidate, &["-c", code]).await {
            Ok((true, stdout, _)) => {
                let version = parse_pyds_version(&stdout);
                return (true, version, Some((*candidate).to_string()));
            }
            _ => continue,
        }
    }
    (false, None, None)
}

/// Strip whitespace/quotes from pyds.__version__ output. Returns None if the
/// stripped string is empty.
fn parse_pyds_version(stdout: &str) -> Option<String> {
    let trimmed = stdout.trim().trim_matches(|c| c == '\'' || c == '"');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Build the install_hint. Sectioned — each section appears only when the
/// corresponding check failed.
fn build_install_hint(status: &SystemStatus) -> String {
    let mut hints: Vec<String> = Vec::new();

    if !status.deepstream_installed {
        hints.push(
            "DeepStream SDK not found. Install DeepStream 7.1 from NVIDIA:\n  \
             https://developer.nvidia.com/deepstream-sdk-7.1\n\
             After install: sudo apt install ./deepstream-7.1_7.1.0-1_amd64.deb"
                .to_string(),
        );
    }

    if !status.gst_plugins_ok && !status.gst_missing.is_empty() {
        let list = status.gst_missing.join(", ");
        hints.push(format!(
            "Missing GStreamer plugins: {list}. These ship with the DeepStream SDK; \
             verify your GST_PLUGIN_PATH points at /opt/nvidia/deepstream/deepstream-7.1/lib/gst-plugins/."
        ));
    }

    if !status.pyds_available {
        hints.push(
            "pyds module not importable. pyds ships bundled with the DeepStream SDK — \
             do NOT `pip install pyds`. Instead, ensure PYTHONPATH includes the \
             DeepStream python bindings (typically \
             /opt/nvidia/deepstream/deepstream-7.1/sources/bindings/python) for the \
             matching Python version."
                .to_string(),
        );
    }

    hints.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tokio::sync::Mutex;

    /// Scripted CommandRunner for unit tests. Keys on the full command string
    /// ("program arg0 arg1 ..."). Unknown keys return `(false, "", "")`.
    pub struct FakeRunner {
        responses: Mutex<HashMap<String, (bool, String, String)>>,
    }

    #[async_trait]
    impl CommandRunner for FakeRunner {
        async fn run(&self, program: &str, args: &[&str]) -> io::Result<(bool, String, String)> {
            let key = std::iter::once(program.to_string())
                .chain(args.iter().map(|s| s.to_string()))
                .collect::<Vec<_>>()
                .join(" ");
            let map = self.responses.lock().await;
            Ok(map
                .get(&key)
                .cloned()
                .unwrap_or((false, String::new(), String::new())))
        }
    }

    impl FakeRunner {
        pub fn new() -> Self {
            Self {
                responses: Mutex::new(HashMap::new()),
            }
        }

        pub async fn set(&self, key: &str, success: bool, stdout: &str, stderr: &str) {
            self.responses.lock().await.insert(
                key.to_string(),
                (success, stdout.to_string(), stderr.to_string()),
            );
        }
    }

    /// Helper: configure the FakeRunner with all checks passing.
    async fn happy_runner() -> FakeRunner {
        let r = FakeRunner::new();
        r.set(
            "sh -c dpkg -l | grep -E '^ii.*deepstream-7'",
            true,
            "ii  deepstream-7.1   7.1.0-1   amd64   NVIDIA DeepStream SDK\n",
            "",
        )
        .await;
        for plugin in REQUIRED_GST_PLUGINS {
            let key = format!("gst-inspect-1.0 {plugin}");
            r.set(&key, true, &format!("{}: exists\n", plugin), "").await;
        }
        r.set(
            "python3.10 -c import pyds; print(pyds.__version__)",
            true,
            "1.1.11\n",
            "",
        )
        .await;
        r
    }

    #[tokio::test]
    async fn all_ok_on_happy_path() {
        let runner = happy_runner().await;
        let status = SystemStatus::run_checks_with(&runner).await;

        assert!(
            status.all_ok(),
            "expected all_ok, got hint:\n{}",
            status.install_hint
        );
        assert_eq!(status.deepstream_version.as_deref(), Some("7.1.0-1"));
        assert_eq!(status.pyds_version.as_deref(), Some("1.1.11"));
        assert_eq!(status.python_bin.as_deref(), Some("python3.10"));
        assert!(status.gst_missing.is_empty(), "no gst plugins missing");
        assert!(status.install_hint.is_empty(), "no hint when all ok");
        assert!(status.deepstream_installed);
        assert!(status.pyds_available);
        assert!(status.gst_plugins_ok);
    }

    #[tokio::test]
    async fn install_hint_mentions_deepstream_7_1_when_ds_missing() {
        let runner = FakeRunner::new();
        // DS dpkg check fails (default for unknown keys).
        for plugin in REQUIRED_GST_PLUGINS {
            let key = format!("gst-inspect-1.0 {plugin}");
            runner.set(&key, true, &format!("{}: exists\n", plugin), "").await;
        }
        runner
            .set(
                "python3.10 -c import pyds; print(pyds.__version__)",
                true,
                "1.1.11\n",
                "",
            )
            .await;

        let status = SystemStatus::run_checks_with(&runner).await;

        assert!(!status.all_ok());
        assert!(!status.deepstream_installed);
        assert!(
            status.install_hint.contains("deepstream-7.1"),
            "hint must reference deepstream-7.1, got: {}",
            status.install_hint
        );
        // Should not include gst/pyds sections since those passed.
        assert!(
            !status.install_hint.contains("GStreamer plugins"),
            "gst hint should be absent"
        );
        assert!(
            !status.install_hint.contains("pyds module"),
            "pyds hint should be absent"
        );
    }

    #[tokio::test]
    async fn gst_missing_lists_each_missing_plugin() {
        let runner = FakeRunner::new();
        runner
            .set(
                "sh -c dpkg -l | grep -E '^ii.*deepstream-7'",
                true,
                "ii  deepstream-7.1   7.1.0-1   amd64   NVIDIA DeepStream SDK\n",
                "",
            )
            .await;
        // nvinfer + nvtracker present.
        runner
            .set("gst-inspect-1.0 nvinfer", true, "nvinfer: exists\n", "")
            .await;
        runner
            .set("gst-inspect-1.0 nvtracker", true, "nvtracker: exists\n", "")
            .await;
        // nvdsanalytics + nvrtspoutsink absent (default unknown-key behavior).
        runner
            .set(
                "python3.10 -c import pyds; print(pyds.__version__)",
                true,
                "1.1.11\n",
                "",
            )
            .await;

        let status = SystemStatus::run_checks_with(&runner).await;

        assert!(!status.gst_plugins_ok);
        assert_eq!(status.gst_missing.len(), 2);
        assert!(
            status.gst_missing.contains(&"nvdsanalytics".to_string()),
            "missing list must include nvdsanalytics: {:?}",
            status.gst_missing
        );
        assert!(
            status.gst_missing.contains(&"nvrtspoutsink".to_string()),
            "missing list must include nvrtspoutsink: {:?}",
            status.gst_missing
        );
        // nvinfer / nvtracker must NOT appear.
        assert!(
            !status.gst_missing.contains(&"nvinfer".to_string()),
            "nvinfer should not be missing"
        );
        assert!(
            !status.gst_missing.contains(&"nvtracker".to_string()),
            "nvtracker should not be missing"
        );
    }

    #[tokio::test]
    async fn python_falls_back_to_python3() {
        let runner = FakeRunner::new();
        runner
            .set(
                "sh -c dpkg -l | grep -E '^ii.*deepstream-7'",
                true,
                "ii  deepstream-7.1   7.1.0-1   amd64   NVIDIA DeepStream SDK\n",
                "",
            )
            .await;
        for plugin in REQUIRED_GST_PLUGINS {
            let key = format!("gst-inspect-1.0 {plugin}");
            runner.set(&key, true, &format!("{}: exists\n", plugin), "").await;
        }
        // python3.10 fails (default), python3 succeeds.
        runner
            .set(
                "python3 -c import pyds; print(pyds.__version__)",
                true,
                "1.1.10\n",
                "",
            )
            .await;

        let status = SystemStatus::run_checks_with(&runner).await;

        assert!(status.pyds_available, "pyds should be available via python3");
        assert_eq!(status.python_bin.as_deref(), Some("python3"));
        assert_eq!(status.pyds_version.as_deref(), Some("1.1.10"));
        assert!(status.all_ok(), "all checks pass even with python3 fallback");
    }

    // --- unit tests for parsers ---

    #[test]
    fn parse_dpkg_line_extracts_version() {
        let v = parse_dpkg_line("ii  deepstream-7.1   7.1.0-1   amd64   NVIDIA DeepStream SDK");
        assert_eq!(v.as_deref(), Some("7.1.0-1"));
    }

    #[test]
    fn parse_dpkg_line_returns_none_for_short_line() {
        assert!(parse_dpkg_line("ii  deepstream").is_none());
        assert!(parse_dpkg_line("").is_none());
    }

    #[test]
    fn parse_pyds_version_strips_quotes_and_whitespace() {
        assert_eq!(parse_pyds_version("1.1.11\n").as_deref(), Some("1.1.11"));
        assert_eq!(parse_pyds_version("  '1.1.11'  ").as_deref(), Some("1.1.11"));
        assert_eq!(parse_pyds_version("\"1.1.11\"").as_deref(), Some("1.1.11"));
        assert!(parse_pyds_version("   ").is_none());
        assert!(parse_pyds_version("").is_none());
    }

    #[test]
    fn install_hint_sections_only_failed_checks() {
        // All-failed status.
        let status = SystemStatus {
            deepstream_installed: false,
            deepstream_version: None,
            pyds_available: false,
            pyds_version: None,
            gst_plugins_ok: false,
            gst_missing: vec!["nvinfer".to_string(), "nvdsanalytics".to_string()],
            python_bin: None,
            last_check_at: 0,
            install_hint: String::new(),
        };
        let hint = build_install_hint(&status);
        assert!(hint.contains("deepstream-7.1"));
        assert!(hint.contains("Missing GStreamer plugins: nvinfer, nvdsanalytics"));
        assert!(hint.contains("pyds module not importable"));
    }

    #[test]
    fn all_ok_false_when_any_check_failed() {
        let make = |ds: bool, pyds: bool, gst: bool| SystemStatus {
            deepstream_installed: ds,
            deepstream_version: None,
            pyds_available: pyds,
            pyds_version: None,
            gst_plugins_ok: gst,
            gst_missing: Vec::new(),
            python_bin: None,
            last_check_at: 0,
            install_hint: String::new(),
        };
        assert!(make(true, true, true).all_ok());
        assert!(!make(false, true, true).all_ok());
        assert!(!make(true, false, true).all_ok());
        assert!(!make(true, true, false).all_ok());
    }
}
