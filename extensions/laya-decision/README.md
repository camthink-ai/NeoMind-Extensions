# LAYA Decision Layer (host)

This extension HOSTS NeoMind's LAYA decision runtime (System-1): a
byte-faithful port of the platform's compiled ONNX engine, served from this
extension's own isolated process on loopback. Installing it IS the
deployment — any build (default server, tauri desktop) gains the decision
layer with no feature rebuild; uninstalling removes it. The platform
discovers the sidecar from the install record and dials it per request, so
the effect is live without a server restart.

- **Engine + sidecar**: loads the artifacts under `<data>/models/decision/`
  (model.onnx | model_int8.onnx | tokenizer/ | remap.json |
  rl_agent_config.json), serves the platform's existing `/v1/systemone`
  protocol on `127.0.0.1:<sidecarPort>` (default 8123; the same config the
  platform reads for the port handshake). Binds loopback only.
- **Honest liveness**: `engine_up` becomes true only after a successful
  bind and is cleared if the serve loop ever exits — status never claims a
  dead engine, and the lazy start (configure/status/metrics) retries.
- **Metrics** (per collect cycle): `model_present`, `model_size_mb`,
  `decisions_logged`, plus the sidecar port/engine state.
- **`status` command**: artifact presence/size, engine/up + port, audit-log
  count, and the last decision record (`logs/decisions.jsonl`).
- **LayaDecisionCard** (dashboard): live mode badge (shadow/guidance),
  backend id, and the authoritative counters via
  `GET /api/decisions/status`. Polls every 10s (configurable).

## Model artifacts (NOT bundled in the .nep)

The .nep ships the ENGINE (extension.dylib + ORT runtime) only. The LAYA
checkpoints are a separate deployment asset (~1.2 GB fp32, ~167 MB int8).
A fresh install needs these under `<data>/models/decision/`:

```
model.onnx + model.onnx.data   # fp32 (default; 100% port fidelity)
   — or —
model_int8.onnx                 # int8 (set engine.int8 in the settings UI)
tokenizer/                      # HF tokenizer (tokenizer.json et al.)
remap.json  rl_agent_config.json
```

Until the files are present the extension installs fine but reports
`model_present: false` / `engine_up: false` (the `status` command's
`install_hint` names exactly this). The engine starts automatically once
the artifacts land — no reinstall needed.

The compiled `decision-native` platform path remains for deployments that
want zero extra processes. Regression eval: `tests/probe_router.py` (16
zh/en probes against the canonical router questions).
