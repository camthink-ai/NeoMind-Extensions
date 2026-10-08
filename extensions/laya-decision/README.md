# LAYA Decision Layer Monitor

Status monitor for NeoMind's built-in LAYA decision layer. The decision
layer itself is compiled into the platform (`decision-native` feature +
ONNX artifacts under `<data>/models/decision/`); this extension makes it
observable — it does NOT implement or replace it.

- **Metrics** (per collect cycle): `model_present`, `model_size_mb`,
  `decisions_logged` — derived from the data dir (`dataDir` config).
- **`status` command**: artifact presence/size, audit-log count, and the
  last decision record (`logs/decisions.jsonl`).
- **LayaDecisionCard** (dashboard): live mode badge (shadow/guidance),
  backend id, and the authoritative counters via
  `GET /api/decisions/status`. Polls every 10s (configurable).
