# Laya Decision Layer (extension-owned)

This extension IS NeoMind's LAYA decision layer — the platform's decision
code has been removed, and this extension is the layer's only home. It hosts
a byte-faithful port of the ONNX decision engine in its own isolated process
and exposes it through two surfaces sharing one engine instance:

- **Commands** (primary): `decide` runs the engine directly (no HTTP hop);
  `status` reports artifact/engine/audit state; `download` fetches missing
  model artifacts. The LLM reaches these through the platform's extension
  command tools — installation is all the wiring there is.
- **Sidecar** (`/v1/systemone` on `127.0.0.1`, config `sidecarPort`, default
  8123): the Jev-compatible HTTP wire API for external or future agent-loop
  consumers.

Installing it IS the deployment (any NeoMind build, no restart);
uninstalling removes the layer.

## decide — presets and custom questions

```jsonc
// routing (default): tool / domain / simple — the canonical router trio
{"state": "User request: 查看所有在线设备的状态"}

// guidance: workflow shape / clarify / scope / skill-guide (v21+ heads)
{"state": "User request: 帮我接入一台新设备", "preset": "guidance"}

// memory: consult-memory timing + injection granularity (v23-line heads;
// zero-shot floor on the v22 checkpoint)
{"state": "User message: 按我之前说的偏好来配置", "preset": "memory"}

// notify: proactive-notification gate (state = event JSON, v23-line head)
{"state": "{\"event\":{\"type\":\"rule_triggered\",\"severity\":\"critical\"},\"detail\":\"temp 85C\"}", "preset": "notify"}

// fully custom questions (Jev wire shape)
{"state": "User request: ...", "questions": {"qid": {"type": "choice", "instructions": "...", "criteria": {"a": "...", "b": "..."}}}}

// ask shorthand: one choice question from bare options
{"state": "User request: ...", "instructions": "Which?", "options": [{"a": "desc a"}, {"b": "desc b"}]}
```

Preset wording is the production contract (LAYA_CHARTER §2): the same text
the fine-tuning pipeline trains on. `routing` works on every checkpoint;
`guidance` needs v21+; `memory`/`notify` heads were trained in the v23 line
and run zero-shot on the deployed v22 checkpoint until that line ships.

## download — artifacts without the manual copy

The .nep ships the ENGINE only. The `download` command fetches the int8
artifact tier (~167MB: `model_int8.onnx` + `tokenizer/` + `remap.json` +
`rl_agent_config.json` + `golden.jsonl`) from the configured `modelBase`
(default: this repo's `decision-v22` GitHub release) into
`<data>/models/decision/`. Retries ×3, temp-file atomic replace,
Content-Length + `.sha256` sidecar verification (when the server provides
one); existing files are never re-downloaded. Progress is per-mille in
`status.download.progress_permille` (9999 = failed) and the metrics channel.
Override the source with the `modelBase` config key (any static file server
hosting the same layout works).

## Monitoring

- **Metrics** per collect cycle: `model_present`, `model_size_mb`,
  `decisions_logged`, download progress.
- **`status` command**: artifact presence/size, engine/up + sidecar port,
  audit count, last decision record, download state, `install_hint` when
  artifacts are missing.
- **LayaDecisionCard** (dashboard): reads `status` via the platform's
  extension-command endpoint — self-sufficient, no platform decision
  endpoints required. Engine badge, artifact size, decision counter; polls
  every 10s (configurable).

## Model artifacts

`<data>/models/decision/` expects `model_int8.onnx` (or `model.onnx` +
`model.onnx.data` for fp32), `tokenizer/`, `remap.json`,
`rl_agent_config.json`. All reads are missing-file safe: no artifacts →
`model_present=false`, never an error.

## Regression

`tests/probe_router.py` — 16 zh/en probes against the canonical router
questions (expects a running sidecar on the configured port).
