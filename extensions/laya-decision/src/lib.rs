//! LAYA Decision Layer extension — the decision runtime, 100% extension-owned.
//!
//! The platform's decision-layer code has been removed (chat hooks, injection
//! modes, the `/api/decisions` endpoints); this extension is the layer's only
//! home. Two surfaces, one engine instance:
//!
//! - **Commands** (the primary path): `decide` runs the engine directly on a
//!   state + optional questions (no HTTP hop); `status` reports artifact
//!   presence, sidecar state, and the audit counter. Callers: the LLM via
//!   extension command tools, agents, and the dashboard card.
//! - **Sidecar** (`/v1/systemone` on 127.0.0.1, config `sidecarPort`,
//!   default 8123): the Jev-compatible wire API for any external or future
//!   agent-loop consumer that prefers HTTP over commands.
//!
//! On configure/status it lazily loads the ONNX checkpoint from
//! `<data>/models/decision/` (model.onnx | model_int8.onnx + tokenizer/ +
//! remap.json + rl_agent_config.json). Any NeoMind build gains the decision
//! layer by installing this extension; uninstalling removes it.
//!
//! Monitoring is unchanged: metrics (artifact presence/size, audit-log
//! count), the `status` command (sidecar port + engine state), and the
//! dashboard card (reads `status` via the extension-command endpoint).
//!
//! All reads are missing-file safe: a deployment without artifacts reports
//! `model_present=false` and serves nothing, never errors.

pub mod downloader;
pub mod laya;
pub mod server;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use neomind_extension_sdk::{
    async_trait, json, Extension, ExtensionCommand, ExtensionError, ExtensionMetadata,
    ExtensionMetricValue, MetricDataType, MetricDescriptor, ParamMetricValue, ParameterDefinition,
    Result,
};

pub struct LayaDecisionExtension {
    data_dir: Mutex<String>,
    /// Model-artifact downloader (progress per-mille on the metrics channel).
    downloader: downloader::Downloader,
    /// Configured artifact base URL (default: GitHub release decision-v22).
    model_base: Mutex<String>,
    request_count: AtomicI64,
    /// Sidecar port (default 8123; config `sidecarPort`). 0 = disabled.
    sidecar_port: Mutex<u16>,
    /// TRUE only while the sidecar is actually accepting connections —
    /// set after a successful bind, CLEARED if the serve loop ever exits,
    /// so status never claims a dead engine and the lazy start retries.
    /// Shared (Arc) with the serve thread for exactly that.
    engine_up: Arc<AtomicBool>,
    /// The loaded engine, shared between the sidecar thread and the
    /// `decide` command (one instance, one memory footprint).
    engine: std::sync::OnceLock<Arc<laya::LayaEngine>>,
    /// Serializes `start_engine`'s check-and-load: configure and the 60s
    /// metrics tick can race into it concurrently — the old check-then-act
    /// double-loaded the ~550MB model and double-spawned the sidecar (the
    /// loser's bind failed and reported an error). Held across the whole
    /// start attempt; contention is irrelevant at this frequency.
    start_lock: Mutex<()>,
}

impl LayaDecisionExtension {
    pub fn new() -> Self {
        Self::with_downloader(downloader::Downloader::new())
    }

    fn with_downloader(dl: downloader::Downloader) -> Self {
        Self {
            downloader: dl,
            model_base: Mutex::new(downloader::DEFAULT_MODEL_BASE.to_string()),
            data_dir: Mutex::new(String::new()), // empty = auto-detect (dir())
            request_count: AtomicI64::new(0),
            sidecar_port: Mutex::new(8123),
            engine_up: Arc::new(AtomicBool::new(false)),
            engine: std::sync::OnceLock::new(),
            start_lock: Mutex::new(()),
        }
    }

    /// Resolve the platform data dir: explicit config > CWD's grandparent
    /// (the host runs extensions with CWD = `<data>/extensions/<id>/`, so
    /// `../..` IS the data dir) > `data` (the platform's CWD-relative
    /// default, for dev runs). Verified against real markers — a stale or
    /// renamed install dir must not silently read some other tree.
    fn dir(&self) -> PathBuf {
        let configured = self.data_dir.lock().unwrap().clone();
        if !configured.is_empty() {
            return PathBuf::from(configured);
        }
        if let Ok(cwd) = std::env::current_dir() {
            let candidate = cwd.join("../..");
            if candidate.join("models").join("decision").is_dir()
                || candidate.join("logs").join("decisions.jsonl").is_file()
            {
                return candidate;
            }
        }
        PathBuf::from("data")
    }

    /// Load the engine from the data dir's artifacts and start the
    /// sidecar server on its own thread. Missing artifacts = monitoring
    /// continues, sidecar stays down (the platform's availability probe
    /// reports it). Idempotent: a second call with the engine already up
    /// only re-binds if the port changed (not supported — restart instead).
    fn start_engine(&self) {
        // Serialize the whole start attempt (see `start_lock`): the
        // engine_up early-exit is only sound when no other caller can be
        // between the check and the bind confirmation.
        let _start = self.start_lock.lock().unwrap();
        if self.engine_up.load(Ordering::SeqCst) {
            return;
        }
        let dir = self.dir().join("models").join("decision");
        let port = *self.sidecar_port.lock().unwrap();
        if port == 0 || !dir.is_dir() {
            return;
        }
        match laya::LayaEngine::load(&dir) {
            Ok(engine) => {
                let engine = Arc::new(engine);
                let _ = self.engine.set(engine.clone());
                let up = self.engine_up.clone();
                // Bind success signal: the thread reports Ok once the
                // listener exists (Err = bind refused, e.g. port taken).
                let (tx, rx) = std::sync::mpsc::channel::<std::result::Result<(), String>>();
                std::thread::Builder::new()
                    .name("laya-sidecar".into())
                    .spawn(move || {
                        match server::bind(port) {
                            Ok(srv) => {
                                let _ = tx.send(Ok(()));
                                let serve_res = server::serve_on(srv, engine);
                                // serve_on returning is catastrophic (or a
                                // tiny_http internal error): stop claiming
                                // the engine is up so the next lazy start
                                // retries instead of lying forever.
                                up.store(false, Ordering::SeqCst);
                                if let Err(e) = serve_res {
                                    eprintln!("laya sidecar server error: {e}");
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(Err(e.clone()));
                                eprintln!("laya sidecar bind failed: {e}");
                            }
                        }
                    })
                    .ok();
                match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                    Ok(Ok(())) => self.engine_up.store(true, Ordering::SeqCst),
                    Ok(Err(e)) => eprintln!(
                        "laya sidecar not started (will retry on next configure/status): {e}"
                    ),
                    Err(_) => eprintln!("laya sidecar bind did not report within 5s"),
                }
            }
            Err(e) => eprintln!("laya engine load failed: {e}"),
        }
    }

    /// Artifact presence + total size of `<data>/models/decision/`.
    fn artifacts(&self) -> (bool, u64) {
        let dir = self.dir().join("models").join("decision");
        let mut total = 0u64;
        let mut present = false;
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for entry in rd.flatten() {
                if let Ok(md) = entry.metadata() {
                    if md.is_file() {
                        present = true;
                        total += md.len();
                    }
                }
            }
        }
        (present, total)
    }

    /// Decision audit-log stats: record count + the last record, parsed
    /// leniently (the file is append-only JSONL; a torn tail line from a
    /// crash mid-write is skipped, not fatal).
    fn audit(&self) -> (i64, Option<serde_json::Value>) {
        let path = self.dir().join("logs").join("decisions.jsonl");
        let Ok(content) = std::fs::read_to_string(&path) else {
            return (0, None);
        };
        let mut count = 0i64;
        let mut last = None;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<serde_json::Value>(line) {
                Ok(v) => {
                    count += 1;
                    last = Some(v);
                }
                Err(_) => continue,
            }
        }
        (count, last)
    }
}

impl Default for LayaDecisionExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for LayaDecisionExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("laya-decision", "LAYA Decision Layer", "2.13.0")
                .with_description(
                    "Status monitor for the LAYA decision layer: artifact presence, \
                     audit-log stats, and the live dashboard card",
                )
                .with_author("NeoMind Team")
                .with_config_parameters(vec![
                ParameterDefinition {
                    name: "dataDir".to_string(),
                    display_name: "Data Directory".to_string(),
                    description: "Platform data directory (holds models/decision/ and logs/decisions.jsonl)".to_string(),
                    param_type: MetricDataType::String,
                    required: false,
                    default_value: Some(ParamMetricValue::String("data".to_string())),
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "sidecarPort".to_string(),
                    display_name: "Sidecar Port".to_string(),
                    description: "Loopback port for the /v1/systemone decision endpoint (0 disables; the platform default expects 8123)".to_string(),
                    param_type: MetricDataType::Integer,
                    required: false,
                    default_value: Some(ParamMetricValue::Integer(8123)),
                    min: None,
                    max: None,
                    options: Vec::new(),
                }])
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricDescriptor {
                name: "model_present".to_string(),
                display_name: "Model Artifacts Present".to_string(),
                data_type: MetricDataType::Boolean,
                unit: String::new(),
                min: None,
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "model_size_mb".to_string(),
                display_name: "Model Artifacts Size".to_string(),
                data_type: MetricDataType::Float,
                unit: "MB".to_string(),
                min: None,
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "decisions_logged".to_string(),
                display_name: "Decisions Logged".to_string(),
                data_type: MetricDataType::Integer,
                unit: String::new(),
                min: None,
                max: None,
                required: false,
            },
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand {
                name: "status".to_string(),
                display_name: "Status".to_string(),
                description: "One JSON summary: artifact presence/size, audit-log count and the last decision record".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: std::collections::HashMap::new(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "download".to_string(),
                display_name: "Download model".to_string(),
                description: "Fetch the missing model artifacts (int8 tier, ~167MB) from the configured modelBase into <data>/models/decision/ — runs in the background; check `status` for download_progress. Files are checksum-verified before landing; existing files are kept.".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: std::collections::HashMap::new(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "decide".to_string(),
                display_name: "Decide".to_string(),
                description: "Run the typed-decision model directly on one state. Pick a preset: routing (tool/domain/simple — call when unsure which tool or CLI domain fits a request), guidance (workflow shape/clarify/scope/skill — call to decide whether to ask, which guide to load), memory (should stored memory be consulted, at what granularity), notify (should this event be pushed to the user now — state = event JSON). Or pass custom questions ({qid: {type, instructions, criteria}}).".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "state".to_string(),
                        description: "The context/question input, exactly as the platform builds it (\"User request: …\"); for the notify preset pass the event JSON string".to_string(),
                        required: true,
                        param_type: MetricDataType::String,
                        ..ParameterDefinition::new("state", MetricDataType::String)
                    },
                    ParameterDefinition {
                        name: "preset".to_string(),
                        description: "Question pack: routing (default) | guidance | memory | notify".to_string(),
                        required: false,
                        param_type: MetricDataType::String,
                        ..ParameterDefinition::new("preset", MetricDataType::String)
                    },
                ],
                fixed_values: std::collections::HashMap::new(),
                samples: vec![
                    json!({"state": "User request: 查看所有在线设备的状态"}),
                    json!({"state": "User request: 我上次说的偏好是什么", "preset": "memory"}),
                    json!({"state": "{\"event\":{\"type\":\"rule_triggered\",\"severity\":\"critical\"},\"detail\":\"temp 85C\"}", "preset": "notify"}),
                    json!({"state": "User request: what alerts need attention?", "questions": {
                        "tool": {"type": "choice", "instructions": "Which tool should serve this user request?", "criteria": {"none": "pure conversation"}}
                    }}),
                ],
                parameter_groups: Vec::new(),
            },
        ]
    }

    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.request_count.fetch_add(1, Ordering::SeqCst);
        match command {
            "decide" => {
                // Standard decision-model command: run the LOCAL engine on
                // one state (+ optional custom questions, wire format
                // identical to POST /v1/systemone). No HTTP hop — the
                // command shares the engine instance with the sidecar.
                self.start_engine();
                let engine = match self.engine.get() {
                    Some(e) => e,
                    None => return Err(ExtensionError::ExecutionFailed(
                        "engine not loaded — check model artifacts under <data>/models/decision/"
                            .to_string(),
                    )),
                };
                let state = args
                    .get("state")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let preset = args
                    .get("preset")
                    .and_then(|v| v.as_str())
                    .unwrap_or("routing")
                    .to_string();
                let questions: std::collections::BTreeMap<String, laya::WireQuestion> = match args
                    .get("questions")
                {
                    Some(q) => serde_json::from_value(q.clone()).map_err(|e| {
                        ExtensionError::InvalidArguments(format!(
                            "bad questions (expect {{qid: {{type, instructions, criteria}}}}): {e}"
                        ))
                    })?,
                    // Ask shorthand (mirrors POST /api/decisions/ask):
                    // state + options [{label: desc} | [labels]] +
                    // optional instructions → one choice question.
                    None if args.get("options").is_some() => {
                        let mut m = std::collections::BTreeMap::new();
                        m.insert(
                            "ask".to_string(),
                            ask_question(
                                args.get("instructions").and_then(|v| v.as_str()),
                                args.get("options"),
                            )?,
                        );
                        m
                    }
                    None => preset_questions(&preset).ok_or_else(|| ExtensionError::InvalidArguments(format!(
                        "unknown preset '{preset}' (routing | guidance | memory | notify)"
                    )))?,
                };
                let started = std::time::Instant::now();
                let answers = engine
                    .decide(&state, &questions)
                    .map_err(ExtensionError::ExecutionFailed)?;
                self.request_count.fetch_add(1, Ordering::SeqCst);
                Ok(json!({
                    "state": state,
                    "answers": answers,
                    "latency_ms": started.elapsed().as_millis() as u64,
                }))
            }
            "download" => {
                // Idempotent background fetch of the int8 artifact tier.
                let dir = self.dir();
                let base = self.model_base.lock().unwrap().clone();
                if self.downloader.active.load(Ordering::SeqCst) == 1 {
                    return Ok(json!({"download": "already-running", "progress_permille": self.downloader.progress.load(Ordering::SeqCst)}));
                }
                if downloader::Downloader::artifacts_present(&dir.join("models").join("decision")) {
                    return Ok(json!({"download": "already-present"}));
                }
                let dl_active = self.downloader.active.clone();
                let dl_progress = self.downloader.progress.clone();
                dl_active.store(1, Ordering::SeqCst);
                dl_progress.store(0, Ordering::SeqCst);
                let target_dir = dir.join("models").join("decision");
                let target_label = target_dir.display().to_string();
                std::thread::spawn(move || {
                    let res = {
                        // one-shot Downloader sharing the shared progress atoms
                        let dl = downloader::Downloader {
                            progress: dl_progress.clone(),
                            active: dl_active.clone(),
                        };
                        dl.ensure_models(&base, &target_dir)
                    };
                    if let Err(e) = res {
                        // surfaced via status: store an error marker in progress
                        // (9999 = failed; status reports the code)
                        dl_progress.store(9999, Ordering::SeqCst);
                        eprintln!("[laya-decision] model download failed: {e}");
                    }
                    dl_active.store(0, Ordering::SeqCst);
                });
                Ok(json!({"download": "started", "target": target_label, "base": self.model_base.lock().unwrap().clone()}))
            }
            "status" => {
                // Lazy start: the host only calls `configure` when a saved
                // config exists (has_config=false skips it), so the engine
                // would never boot on a fresh install. Status is the other
                // guaranteed early touch.
                self.start_engine();
                let (present, size) = self.artifacts();
                let (logged, last) = self.audit();
                let port = *self.sidecar_port.lock().unwrap();
                Ok(json!({
                    "model_present": present,
                    "model_size_bytes": size,
                    "decisions_logged": logged,
                    "last_decision": last,
                    // The .nep ships the ENGINE only — model artifacts (~1.2GB
                    // fp32 / ~167MB int8) are a separate deployment asset. A
                    // fresh install with no artifacts used to report bare
                    // booleans with no route forward; this hint names the
                    // expected files and where they go.
                    "install_hint": if present { serde_json::Value::Null } else { serde_json::json!(
                        "Model artifacts MISSING — run the download command to fetch the int8 tier (~167MB, checksum-verified) from the configured modelBase, or place the v22 checkpoint set (model_int8.onnx + tokenizer/ + remap.json + rl_agent_config.json) under <data>/models/decision/ manually. The engine starts automatically once the files are present."
                    )},
                    "download": {
                        "progress_permille": self.downloader.progress.load(Ordering::SeqCst),
                        "active": self.downloader.active.load(Ordering::SeqCst) == 1,
                        "model_base": self.model_base.lock().unwrap().clone(),
                    },
                    "sidecar": {
                        "port": port,
                        "url": format!("http://127.0.0.1:{port}"),
                        "engine_up": self.engine_up.load(Ordering::SeqCst),
                    },
                }))
            }
            "configure" => {
                if let Some(d) = args.get("dataDir").and_then(|v| v.as_str()) {
                    *self.data_dir.lock().unwrap() = d.to_string();
                }
                if let Some(p) = args.get("sidecarPort").and_then(|v| v.as_u64()) {
                    *self.sidecar_port.lock().unwrap() = p as u16;
                }
                self.start_engine();
                Ok(json!({"status": "ok", "engine_up": self.engine_up.load(Ordering::SeqCst)}))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        self.start_engine();
        let (present, size) = self.artifacts();
        let (logged, _) = self.audit();
        Ok(vec![
            ExtensionMetricValue {
                name: "model_present".to_string(),
                value: ParamMetricValue::Boolean(present),
                timestamp: 0,
            },
            ExtensionMetricValue {
                name: "model_size_mb".to_string(),
                value: ParamMetricValue::Float((size as f64) / 1_048_576.0),
                timestamp: 0,
            },
            ExtensionMetricValue {
                name: "decisions_logged".to_string(),
                value: ParamMetricValue::Integer(logged),
                timestamp: 0,
            },
        ])
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

neomind_extension_sdk::neomind_export!(LayaDecisionExtension);

/// Build one ad-hoc choice question from the ask shorthand. Accepts
/// ["a","b"] or {"a": "desc"}; errors when fewer than 2 options resolve —
/// same contract as POST /api/decisions/ask.
fn ask_question(
    instructions: Option<&str>,
    options: Option<&serde_json::Value>,
) -> std::result::Result<laya::WireQuestion, ExtensionError> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    match options {
        Some(serde_json::Value::Array(arr)) => {
            for v in arr {
                if let Some(l) = v.as_str() {
                    pairs.push((l.to_string(), l.to_string()));
                }
            }
        }
        Some(serde_json::Value::Object(map)) => {
            for (k, v) in map {
                pairs.push((k.clone(), v.as_str().unwrap_or(k).to_string()));
            }
        }
        _ => {}
    }
    if pairs.len() < 2 {
        return Err(ExtensionError::InvalidArguments(
            "options must resolve to ≥2 entries: [\"a\",\"b\"] or {\"label\": \"description\"}"
                .to_string(),
        ));
    }
    let mut criteria = serde_json::Map::new();
    for (l, d) in pairs {
        criteria.insert(l, serde_json::Value::String(d));
    }
    Ok(laya::WireQuestion {
        kind: laya::QuestionKind::Choice,
        instructions: instructions
            .unwrap_or("Which option best matches the user request?")
            .to_string(),
        criteria: serde_json::Value::Object(criteria),
    })
}

/// The canonical router questions, duplicated from the platform contract
/// (crates/neomind-api/src/handlers/decisions.rs TOOL_*/SIMPLE_*/DOMAIN_*).
/// DRIFT WARNING: these strings are a training contract — if the platform
/// rewords them, update here (and tests/probe_router.py) or this default
/// evaluates a different task than production routing.
fn default_router_questions() -> std::collections::BTreeMap<String, laya::WireQuestion> {
    use laya::QuestionKind;
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        "tool".to_string(),
        laya::WireQuestion {
            kind: QuestionKind::Choice,
            instructions: "Which tool should serve this user request?".to_string(),
            criteria: serde_json::json!({
                "shell": "ALL neomind CLI platform operations, INCLUDING device control commands (reboot a device, send downlink), rules, agents config, dashboards, transforms, messages, extensions, purge/export/import",
                "query_conclusion": "read an AI agent's most recent conclusion — instant and free ('is X ok', 'what did the inspection say')",
                "run_now": "trigger a FRESH run of an AI AGENT right now — only for agents; device commands (e.g. reboot a sensor) are shell, not this",
                "skill": "load a step-by-step workflow guide for a complex or unfamiliar domain (onboarding, development guides)",
                "memory": "persist or recall cross-conversation facts and preferences ('remember…', 'forget…', 'what do you know about me')",
                "vision": "understand, read or describe the CONTENT of an image — what is in it, gauge readings, text, anomalies; no pixel changes",
                "image_edit": "modify image pixels: crop, draw, blur, annotate, highlight — changes the picture, does not interpret it",
                "web_fetch": "fetch the content of an external URL",
                "file_write": "create or overwrite a WHOLE file with new content",
                "file_edit": "surgically replace an exact existing string inside an existing file, keeping the rest untouched",
                "none": "pure conversation — greeting, identity, thanks, chat with no platform entity"
            }),
        },
    );
    m.insert(
        "simple".to_string(),
        laya::WireQuestion {
            kind: QuestionKind::Noul,
            instructions: "Can this request be completed with a single direct command or lookup, without multi-step reasoning, correlation or drafting?".to_string(),
            criteria: serde_json::Value::Null,
        },
    );
    m.insert(
        "domain".to_string(),
        laya::WireQuestion {
            kind: QuestionKind::Choice,
            instructions: "Which NeoMind CLI domain (or special path) should handle the user request?".to_string(),
            criteria: serde_json::json!({
                "device": "inspect, onboard, control or configure devices and read their metrics",
                "rule": "create, inspect or manage automation rules (threshold alerts, event triggers)",
                "message": "send notifications or manage notification channels and alert history",
                "extension": "install, configure or invoke extension capabilities (vision AI, bridges)",
                "agent": "create, run or manage scheduled AI agents",
                "dashboard": "create or edit dashboards and bind data sources to cards/widgets",
                "transform": "create or test data transforms (computed/virtual metrics)",
                "widget": "develop or manage community dashboard widgets",
                "connector": "manage protocol connectors (Modbus, OPC-UA, BACnet, LoRaWAN)",
                "push": "configure forwarding telemetry to external systems (data push)",
                "llm": "configure or switch LLM backends",
                "settings": "platform settings (retention, language, system configuration)",
                "system": "system diagnostics, health, logs, version info",
                "image_edit": "edit or annotate an image (crop, draw, blur)",
                "converse": "plain conversation, greeting, opinion or summary needing no platform tool"
            }),
        },
    );
    m
}

/// Question-preset packs for `decide --preset`. Wording is the production
/// contract (LAYA_CHARTER §2): trained in the fine-tuning pipeline, mirrored
/// here. `routing` works on every checkpoint; `guidance` needs v21+; the
/// `memory`/`notify` heads were trained in the v23 line — on the deployed
/// v22 checkpoint they run zero-shot (usable floor, not gate-grade).
fn preset_questions(preset: &str) -> Option<std::collections::BTreeMap<String, laya::WireQuestion>> {
    use laya::QuestionKind;
    let mut m = std::collections::BTreeMap::new();
    match preset {
        "routing" => return Some(default_router_questions()),
        "guidance" => {
            m.insert("workflow".to_string(), laya::WireQuestion {
                kind: QuestionKind::Choice,
                instructions: "What shape of work does this request need?".into(),
                criteria: serde_json::json!({
                    "direct": "exactly one command, lookup, or question",
                    "multi_step": "two or more operations chained or spread across turns",
                    "guided": "an interactive guided setup flow",
                    "converse": "pure conversation, no platform operations"
                }),
            });
            m.insert("clarify".to_string(), laya::WireQuestion {
                kind: QuestionKind::Noul,
                instructions: "Does this request need more details from the user before acting?".into(),
                criteria: serde_json::Value::Null,
            });
            m.insert("scope".to_string(), laya::WireQuestion {
                kind: QuestionKind::Choice,
                instructions: "What is the scope of this request?".into(),
                criteria: serde_json::json!({
                    "neomind_op": "platform operations",
                    "neomind_setup": "initial setup/onboarding",
                    "general_chat": "general conversation",
                    "out_of_scope": "unrelated to the platform"
                }),
            });
            m.insert("skill".to_string(), laya::WireQuestion {
                kind: QuestionKind::Choice,
                instructions: "Which workflow guide (if any) should be loaded for this request?".into(),
                criteria: serde_json::json!({
                    "agent-management": "create, run or manage scheduled AI agents",
                    "connector-management": "manage protocol connectors (Modbus, OPC-UA, BACnet, LoRaWAN)",
                    "dashboard-management": "create or edit dashboards and bind data sources",
                    "data-push-management": "configure forwarding telemetry to external systems",
                    "device-onboarding": "connect and register new devices step by step",
                    "extension-development": "build custom extensions",
                    "extension-management": "install, configure or invoke extensions",
                    "llm-management": "configure or switch LLM backends",
                    "message-management": "send notifications or manage channels and alert history",
                    "rule-management": "create, inspect or manage automation rules",
                    "settings-management": "platform settings (retention, language, configuration)",
                    "system-info": "system diagnostics, health, logs, version info",
                    "transform-management": "create or test data transforms (computed metrics)",
                    "widget-development": "develop community dashboard widgets",
                    "widget-management": "manage dashboard widgets",
                    "none": "no guide needed — direct operation or conversation"
                }),
            });
        }
        "memory" => {
            m.insert("timing".to_string(), laya::WireQuestion {
                kind: QuestionKind::Noul,
                instructions: "Should stored user memory (preferences, prior facts, procedures) be consulted before answering this message?".into(),
                criteria: serde_json::Value::Null,
            });
            m.insert("granularity".to_string(), laya::WireQuestion {
                kind: QuestionKind::Choice,
                instructions: "How much stored memory should be attached to this message's context?".into(),
                criteria: serde_json::json!({
                    "none": "no memory needed — the message is self-contained",
                    "preferences": "the user's stated preferences and personal facts only",
                    "knowledge": "the full relevant knowledge file or procedure guide"
                }),
            });
        }
        "notify" => {
            m.insert("notify".to_string(), laya::WireQuestion {
                kind: QuestionKind::Noul,
                instructions: "A platform event just occurred. Should the user be proactively notified about it right now?".into(),
                criteria: serde_json::Value::Null,
            });
        }
        _ => return None,
    }
    Some(m)
}
