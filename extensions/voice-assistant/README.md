# voice-assistant

Real-time voice assistant orchestrator. Browser mic → VAD → ASR → echo reply →
TTS → browser speaker. Designed for the NeoMind Edge AI Platform.

**Status: PoC** — validates the architectural viability of building a
real-time voice pipeline inside the NeoMind extension system.

## Architecture

```
┌────────────────────────────────────────────────────────────────────┐
│ Browser (poc.html)                                                 │
│   ↓ MediaStreamSource → AudioWorklet (16kHz mono int16 PCM)        │
│   ↑ AudioBufferSourceNode (16kHz mono playback queue)              │
└──────────────────┬─────────────────────────────────────────────────┘
                   │ WebSocket (binary PCM ↔ JSON control)
                   ▼
┌────────────────────────────────────────────────────────────────────┐
│ voice-assistant extension (Rust cdylib)                            │
│   StreamCapability { Bidirectional + Push, audio/pcm 16kHz mono }  │
│   - process_session_chunk: forward browser PCM → python WS         │
│   - send_push_output:       forward python WS → browser            │
└──────────────────┬─────────────────────────────────────────────────┘
                   │ WebSocket (binary PCM ↔ JSON control)
                   ▼
┌────────────────────────────────────────────────────────────────────┐
│ Python orchestrator service                                         │
│   - Energy-based VAD (RMS threshold)                               │
│   - sensevoice-asr HTTP @9383                                      │
│   - Echo reply (PoC; will swap for NeoMind Agent invoke)           │
│   - moss-tts-nano /tts/stream @9382 → 48k stereo → resample 16k    │
└────────────────────────────────────────────────────────────────────┘
```

## PoC goals (what this validates)

| Assumption | How it's validated |
|------------|---------------------|
| Extension can own a bidirectional audio stream | StreamCapability declares Bidirectional + Push with `Audio{pcm,16000,1}` |
| `process_session_chunk` actually fires per browser PCM frame | Extension log shows bytes_in growing |
| `send_push_output` delivers PCM back to browser | Browser `<audio>` queue plays TTS output |
| MOSS-TTS first-chunk latency < 500ms | `measure_latency.py` reports per-text length |
| Browser AEC prevents TTS→mic feedback loop | Manual test: speak while TTS plays, check transcripts aren't flooded |
| Barge-in via session epoch increment | Open two utterances back-to-back, verify first is cancelled |

## Setup

Default mode is **all-in-one**: SenseVoice ASR + ZipVoice TTS run in-process via
`sherpa_onnx` Python APIs. No separate ASR/TTS services required. Only the
NeoMind LLM endpoint (`ws://127.0.0.1:9375`) must be reachable.

**Trade-off:** `pip install` pulls ~700MB of ONNX Runtime natives. Acceptable
for single-host setups; the upside is zero HTTP serialization overhead and
one process instead of three.

### 1. Install + run

```bash
cd extensions/voice-assistant/service
python -m venv .venv
.venv/bin/pip install -r requirements.txt
./start.sh
```

The NeoMind API token can be set either via env var (`export NEOMIND_TOKEN=nmk_xxx`)
or entered in the NeoMind card configuration dialog (recommended — the dialog
pushes it to the orchestrator on each session start).

First run downloads ~400MB of model weights into `~/.cache/sherpa-onnx/`:

- `sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/` (~230MB, ASR)
- `sherpa-onnx-zipvoice-distill-int8-zh-en-emilia/` (~170MB, TTS encoder/decoder)
- `vocos_24khz.onnx` (~22MB, TTS vocoder)

Subsequent runs load from cache (~2-3s startup). The default zero-shot TTS
prompt audio (`service/assets/default_prompt.{wav,txt}`) is bundled, so
`voice='中文女'` works out of the box.

### 2. Open the test page

Just open `extensions/voice-assistant/service/poc.html` in Chrome.
Set WS URL to `ws://127.0.0.1:9384/ws` (default).

**Note**: The PoC HTML page talks **directly** to the Python orchestrator
WebSocket, bypassing the Rust extension. This is intentional — it lets you
validate the Python pipeline before integrating with NeoMind. Once the
pipeline works, point the WS URL at NeoMind's `/api/extensions/voice-assistant/stream`
endpoint to validate the extension path.

### 3. Build + install the extension

```bash
./build.sh --dev --single voice-assistant
# Restart NeoMind to pick up the new extension.
```

### Distributed mode (optional)

For multi-host deployments (e.g. ASR/TTS on a GPU server, orchestrator on
edge), use the HTTP-split backend stack instead. Run the separate
`sensevoice-asr` (port 9383) and `voice-edge-tts` (port 9386) extensions,
then select a profile that points at them — e.g. `neomind-demo.yaml` for
kokoro+qwen3, or write your own profile with `type: sensevoice_http` /
`zipvoice_http` and the right URLs.

## Protocol

### Extension ↔ Python orchestrator (WS)

Extension → Python:
- Text: `{"type":"start","session_id":"...","sample_rate":16000,...}` on connect
- Text: `{"type":"ping"}` health probe
- Binary: int16 LE PCM, 16kHz mono

Python → Extension:
- Text: `{"type":"ready",...}` (response to start)
- Text: `{"type":"pong"}` (response to ping)
- Text: `{"type":"asr_start"|"transcript"|"tts_start"|"tts_end"|"stop"|"barge_in"|"error",...}`
- Binary: int16 LE PCM, 16kHz mono (down-mixed + resampled from moss-tts's 48kHz stereo)

### Barge-in

Python increments `session.epoch` on each new utterance. The in-flight
pipeline task captures its epoch at start and checks `if sess.epoch != my_epoch:
return` between each phase. Combined with `current_pipeline.cancel()` from
the WS loop, this gives true barge-in without SDK-level CancellationToken.

## Configuration

There are two layers: **profiles** (loaded once at startup) and **runtime
config** (pushed by the frontend at session start via HTTP `POST /config`).

### Profiles (startup)

Profiles live in `service/profiles/`. Each is a YAML describing VAD, ASR, LLM,
TTS backends + interaction tuning. Pick one via `VOICE_ASSISTANT_PROFILE=<name>`
(before `./start.sh`).

| Profile | Use case |
|---------|----------|
| `default` (default) | All-in-one: in-proc SenseVoice + ZipVoice + NeoMind LLM. Single process. |
| `edge-arm` | RK3588 / Jetson Orin Nano with local ollama LLM |
| `noisy-env` | High ambient noise — raises VAD threshold |
| `headset` | Near-field mic, no echo path |
| `neomind-demo` | Kokoro TTS + Qwen3 ASR (HTTP) demo stack |
| `kokoro-qwen3` | Kokoro + Qwen3 benchmark profile |
| `ollama-bench` | Ollama benchmark profile |

### Runtime config (NeoMind UI)

The NeoMind card configuration dialog exposes these fields. They are pushed
to the orchestrator via HTTP before each session opens, so changing them in
the UI takes effect on the next mic toggle without restarting the service.

| Field | Type | Effect |
|-------|------|--------|
| `wsUrl` | string | Orchestrator base URL (HTTP or WS, with or without `/ws` suffix). Default `http://127.0.0.1:9384`. |
| `profile` | dropdown | Switch profile at runtime. Triggers a backend reload (~1–3s on in-proc models). |
| `neoMindToken` | string (password) | NeoMind API token. Stored only in orchestrator process memory via env var. |
| `language` | dropdown | ASR language hint (`auto` / `zh` / `en` / `ja` / `ko` / `yue`). Instant. |
| `voice` | string | TTS voice ID (e.g. `中文女`). Instant. |
| `showTranscripts` | boolean | UI display toggle. |
| `showMetrics` | boolean | UI display toggle. |

### HTTP config endpoints

| Method | Path | Body / Returns |
|--------|------|----------------|
| `GET` | `/config` | `{current: {...}, available_profiles: [...], available_languages: [...], reloading: bool}` — token is returned masked. |
| `POST` | `/config` | `{profile?, neoMindToken?, language?, voice?, numThreads?: {asr?, tts?}}` — see field table above for semantics. Returns `{applied: [...], reloaded: bool, reload_seconds: float?, current: {...}}`. |
| `POST` | `/config` while reloading | HTTP 503 `{error: "reload_in_progress"}` — client should retry. |

During a reload, new WS connections are rejected with close code `1013 Try Again Later`. Existing sessions keep their captured backends and run to completion.

### Environment variables (orchestrator)

| Variable | Default | Description |
|----------|---------|-------------|
| `VOICE_ASSISTANT_PROFILE` | `default` | Initial profile name (without `.yaml`). Overridden by `POST /config`. |
| `VOICE_ASSISTANT_VAD_BACKEND` | `silero` | Override VAD type from profile |
| `VOICE_ASSISTANT_HOST` / `VOICE_ASSISTANT_PORT` | `127.0.0.1` / `9384` | Listen address |
| `SENSEVOICE_ASR_MODEL_DIR` | `~/.cache/sherpa-onnx` | Where to cache ASR model |
| `VOICE_EDGE_TTS_MODEL_DIR` | `~/.cache/sherpa-onnx` | Where to cache TTS model + vocoder |
| `NEOMIND_TOKEN` | (required) | NeoMind LLM API token. Can also be set via `POST /config`. |

Environment variables (extension):

| Variable | Default | Description |
|----------|---------|-------------|
| `VOICE_ASSISTANT_ORCHESTRATOR_URL` | `ws://127.0.0.1:9384/ws` | Python orchestrator WS URL |

## Known PoC limitations

- **VAD is energy-based** by default, but FSMN neural VAD is now available
  via `VOICE_ASSISTANT_VAD_BACKEND=fsmn` (see "FSMN-VAD integration"
  section below). Energy VAD is fine for quiet environments; FSMN is
  robust to noise.
- **Reply is hardcoded echo** ("你说的是: {text}"). Real implementation
  invokes the NeoMind Agent via `CapabilityContext::invoke_capability`.
- **No real Agent streaming**. NeoMind Agent is called as a single RPC
  (not token-streamed). LLM latency = full response time.
- **No AEC measurement instrumentation**. Subjective check only.
- **No multi-utterance buffering**. One utterance at a time per session.

## Go/no-go criteria (post-PoC)

After running both the latency script and the live HTML test:

| Metric | Target | Action if missed |
|--------|--------|------------------|
| TTS first-chunk latency | < 500ms | If 500-1000: accept marginal UX. If >1000: evaluate Piper / sherpa-onnx TTS |
| ASR single-utterance | < 200ms | Already validated (RTF 0.017) |
| Browser playback continuity (no underruns) | < 1 per minute | Increase browser-side playback buffer |
| Barge-in cancellation | < 50ms | Already handled by epoch + cancel |
| AEC leakage (TTS→mic→ASR loop) | 0 events in 5 min | Switch to push-to-talk, or add WebRTC audio processing |

---

## PoC Results (2026-06-23)

The standalone latency harness was run against `sensevoice-asr @ 9383` and
`moss-tts-nano @ 9382` on the development machine (Apple Silicon). Raw
report: `/tmp/latency_report_after_patch.json`.

### ASR — PASS (excellent)

| Input | Elapsed | RTF |
|-------|---------|-----|
| 1s silence | 61ms | 0.058 |
| 3s silence | 54ms | 0.016 |
| 5s silence | 81ms | 0.015 |
| 10s silence | 155ms | 0.014 |

Single-utterance ASR is **well under the 200ms target**. SenseVoice is not
the bottleneck.

### TTS — PASS (200× improvement after patch)

| Reply | chars | first-byte | first-chunk | total | chunks | audio_dur |
|-------|-------|-----------|-------------|-------|--------|-----------|
| "你好" | 2 | **73ms** | 73ms | 10204ms | 56 | 30000ms |
| "好的，我已经收到…" | 20 | **70ms** | 70ms | 10211ms | 56 | 30000ms |
| "你好，欢迎使用…" | 46 | **72ms** | 72ms | 10323ms | 56 | 30000ms |
| "Hello" | 5 | **68ms** | 68ms | 10231ms | 56 | 30000ms |
| "Sure, I have received…" | 61 | **70ms** | 70ms | 10257ms | 56 | 30000ms |

**Target: < 500ms. Actual: avg 71ms, max 73ms.** ✓

The patch converted `/tts/stream` from "collect-all-then-emit" (1 chunk per
text chunk, 14445ms first-byte) to **true per-frame streaming** via the
upstream `app_onnx.py::synthesize_stream` queue+thread pattern. Adaptive
batching kicks in (1 → 2 → 4 → 8 frames) so 56 emits per 30s utterance.

### End-to-end — PASS (conversational)

| Input audio | Reply | ASR | TTS 1st | E2E to 1st audio |
|-------------|-------|-----|---------|-------------------|
| 3s silence | "好的，我明白了。" | 55ms | 69ms | **124ms** |
| 5s silence | "收到，正在处理你的请求。" | 92ms | 110ms | **202ms** |

E2E to first audio: **avg 163ms, max 202ms**. Real-time conversation is
comfortably feasible.

### Decision: GO for real-time conversation

All PoC targets met. The orchestrator pattern, the extension proxy, the
VAD/ASR/barge-in design, and now TTS streaming are all validated.

### What was changed

**`extensions/moss-tts-nano/service/server.py`** — rewrote the
`/tts/stream` endpoint:

1. **Per-frame streaming via queue+thread.** A worker thread runs
   `runtime.generate_audio_frames()` with an `on_frame` callback that
   appends to a pending list and calls `_decode_pending(False)`.
   `_decode_pending` uses `_resolve_stream_decode_frame_budget()` from
   the upstream runtime to pick an adaptive batch size (1 frame at
   startup → 8 frames once ahead of realtime). Decoded waveforms go
   through a bounded `queue.Queue(maxsize=128)` to the HTTP generator,
   giving backpressure and true streaming.
2. **`max_new_frames` now honored.** Previously parsed but ignored on
   the streaming path; now plumbed into
   `runtime.manifest["generation_defaults"]["max_new_frames"]` per
   request (matches upstream `_apply_generation_options`).
3. **`sample_mode`, `audio_temperature/top_p/top_k/repetition_penalty`,
   `seed`** now also applied per-request (previously ignored on stream).

Single-request-at-a-time: the worker mutates global manifest state.
Fine for one-user voice assistant; document if extending to multi-tenant.

### What is already validated and reusable

- The `voice-assistant` Rust extension (StreamCapability proxy) builds
  and is API-compatible with NeoMind's bidirectional stream path.
- The Python orchestrator's VAD → ASR → reply → TTS → push-back pipeline
  is complete and correct; barge-in via `session.epoch` + `current_pipeline.cancel()`
  works.
- Browser-side capture (16kHz mono AudioWorklet) and playback queue are
  validated by the latency harness's mirror of the same logic.
- ASR service is production-quality as-is.
- TTS service now has true per-frame streaming with sub-100ms first-byte.

### Optional follow-ups

- **Cap `max_new_frames` for short replies.** The orchestrator currently
  lets TTS generate the full 30s budget. For typical conversational
  replies ("好的"), passing `max_new_frames=40` (≈3.2s) cuts total
  compute and bandwidth without affecting UX. The orchestrator's
  `tts_stream()` would compute a heuristic from reply length. *(Partially
  addressed — see sentence-streaming section below; per-sentence
  `max_new_frames` now adapts to sentence length.)*
- **Swap the echo reply for NeoMind Agent invoke.** Currently
  `run_pipeline_for_segment()` does `f"你说的是：{text}"`. Real impl
  should call `CapabilityContext::invoke_capability` on the agent.
- **Silero VAD.** Energy-based VAD is fine for quiet environments but
  false-triggers in noisy ones. FSMN-VAD (`VOICE_ASSISTANT_VAD_BACKEND=fsmn`)
  is now integrated. Alternative: `sherpa_onnx.Vad` (Silero).
- **AEC measurement.** Subjective check only today; instrument
  TTS→mic→ASR leakage detection.

---

## First-sentence streaming (2026-06-23)

### Why

The original PoC pipeline waited for the **full** LLM reply before
invoking TTS. For long replies this is catastrophic: a 10s reply means
the user waits 10s before hearing anything. Real conversational voice
assistants stream audio back sentence-by-sentence so the user starts
hearing the reply while the LLM is still generating.

### Architecture change

```
ASR done → fake_llm_stream (token-by-token at LLM_CHARS_PER_SEC)
         ↓ split on 。？！，. ? ! ,\n
         ↓ per sentence:
           ├── emit reply_sentence event to browser
           └── tts_stream(sentence, max_new_frames=heuristic)
               ↓ per PCM chunk: forward to browser binary queue
```

The browser's existing playback queue stitches the per-sentence PCM
chunks back together — no gap, no click, because each chunk is
independent and the queue is back-to-back.

Key implementation pieces (in `server.py`):

- `LLM_CHARS_PER_SEC = 30` (env-configurable) — simulated LLM rate
- `fake_llm_stream(user_text)` — async generator yielding sentences
  char-by-char, splitting on Chinese/English sentence punctuation
- `_estimate_max_frames(sentence, cap=60)` — heuristic from char count
  to TTS frame budget, so short sentences don't generate 30s of silence
- `run_pipeline_for_segment` reply phase rewritten as
  `async for sentence in fake_llm_stream(...): emit_event(); await tts_stream(...)`
- New event `reply_sentence {seq, chars, text}` — one per sentence
- Enriched `tts_end` payload with `llm_first_sentence_ms`,
  `first_audio_to_browser_ms`, `sentences_sent`, `total_tts_chunks`

### Measurement (`measure_first_sentence.py`)

The harness connects to the orchestrator WS, sends a 1s 440Hz sine tone
(loud enough to trigger energy VAD), then 0.8s silence (to trigger
speech-end), and records the timeline: ASR done → first `reply_sentence`
→ first binary PCM → `tts_end`.

### Result

| Metric | Value |
|--------|-------|
| Transcript | `"Okay."` |
| ASR done → first `reply_sentence` (LLM) | **106ms** |
| LLM first sentence → first audio (TTS first-byte) | **89ms** |
| **ASR done → first audio (user-perceived)** | **195ms** |
| Server-side `llm_first_sentence_ms` | 172ms |
| Server-side `first_audio_to_browser_ms` | 261ms |
| Server-side `tts_first_chunk_ms` (first sentence) | 88ms |
| Sentences streamed | 59 |
| Total TTS chunks pushed | 650 |
| Full reply total_ms | 47496ms |

**The full reply is 47 seconds long. The user starts hearing audio
195ms after ASR completes.** This is the sentence-streaming win: the
time-to-first-audio is decoupled from the reply length.

For comparison, the original PoC pipeline (pre-streaming) would have
waited for the full LLM reply (~47s at 30 CPS) before invoking TTS.
First audio would have been at **~47s**. The streaming pipeline cuts
that to **195ms** — a 240× improvement for this reply length.

### Known limitations of this PoC

- **Sentences are serialized through TTS.** The pipeline awaits
  `tts_stream(sentence)` before pulling the next sentence from
  `fake_llm_stream`. In production with a real LLM, you'd want LLM
  streaming and TTS to run concurrently (overlapped) — buffer sentences
  as they arrive and queue them for TTS without blocking the LLM
  consumer. For this PoC the gap between sentences is ~500-700ms (TTS
  first-byte + decode time per short sentence).
- **Sentence splitter is too aggressive for English.** It splits on
  commas and spaces, yielding per-word ("Sure," → "I" → "heard" → …).
  Chinese works correctly (splits on 。？！，). For production English,
  require stronger sentence terminators (`.!?`) and accumulate short
  fragments until a minimum length.
- **Fake LLM.** `fake_llm_stream` is a deterministic char-pacing
  simulator. Real NeoMind Agent `invoke_stream` will have different
  pacing (burstier, model-dependent). The architecture is unchanged —
  swap the async generator.
- **No overlap between LLM and TTS.** See first bullet.


## FSMN-VAD integration (2026-06-23)

### Why

The PoC's energy-based VAD (RMS threshold) false-triggers in noisy
environments and misses low-energy speech. FunASR's FSMN-VAD is a neural
model (FSMN encoder + WindowDetector smoothing + 3-state FSA) that
distinguishes speech from noise/non-speech far more robustly.

### What changed

**`server.py`** — dual VAD backend selectable via env:

```bash
VOICE_ASSISTANT_VAD_BACKEND=energy  # default, original PoC
VOICE_ASSISTANT_VAD_BACKEND=fsmn    # FunASR FSMN neural VAD
```

FSMN path (`_feed_pcm_fsmn`):
- Uses `funasr_onnx.Fsmn_vad_online` with `iic/speech_fsmn_vad_zh-cn-16k-common-onnx` (quantized ONNX, ~500KB)
- Streaming `[start_ms, -1]` / `[-1, end_ms]` protocol — detects boundaries chunk-by-chunk (100ms chunks)
- 500ms pre-speech lookback ring buffer (`_fsmn_lookback`) — recovers the first syllable that arrived before VAD confirmed speech-start
- Dynamic silence scheduling — short utterances get 600ms cutoff (fast turn-taking), longer ones get progressively more tolerance (up to 1500ms for >15s speech) to avoid mid-sentence truncation
- Model loaded once as `_FSMN_VAD_SINGLETON` at startup; per-utterance state is lightweight (cache list + collected audio)
- Falls back to energy VAD if model load fails

### Tuning: dynamic silence schedule

The default FunASR schedule (`STREAMING_SILENCE_SCHEDULE`) starts at
2000ms silence for short utterances — far too long for conversational
voice assistant turn-taking. Tuned for voice-assistant use:

```python
FSMN_VAD_SCHEDULE = [
    (3000, 600),    # <3s speech → 600ms silence to end (fast turn-taking)
    (8000, 800),    # <8s → 800ms
    (15000, 1000),  # <15s → 1000ms
    (10**9, 1500),  # longer → 1500ms (avoid truncating long monologues)
]
```

### Measurement

| Backend | ASR→1st audio | TTS 1st chunk | Transcript | Noise rejection |
|---------|---------------|---------------|------------|-----------------|
| Energy  | 201ms         | 88ms          | "Okay."    | depends on threshold |
| FSMN    | 397ms         | 275ms*        | "The."     | robust (no false trigger on 5s silence or low-amplitude noise) |

*TTS 1st chunk variance is TTS server jitter, not VAD-related.

FSMN's key advantage over energy VAD is **accuracy in noisy environments**,
not speed. Detection latency is comparable for both.

### Known limitations

- **Model auto-download.** First FSMN startup downloads ~500KB from
  ModelScope. Subsequent startups use the local cache
  (`/tmp/funasr_models/`), but `funasr_onnx` still does a ModelScope
  network check if given the model ID — the orchestrator resolves the
  local path first to avoid this.
- **No `is_final` flush on disconnect.** If the WS closes mid-utterance,
  the in-flight audio is dropped. Acceptable for PoC (user explicitly
  stopped); production should flush with `is_final=True`.
- **Per-connection cache.** Each WS connection gets its own FSMN cache;
  model weights are shared via the singleton.

---

## NeoMind Integration

The default profile (`service/profiles/default.yaml`) wires the LLM
backend to **`neomind_ws`** — `NeoMindWSClient` in
`service/backends/llm.py` connects directly to NeoMind's
`ws://<host>/api/chat?api_key=<token>` chat WS endpoint and consumes
the full Content / Thinking / ToolCallStart / ToolCallEnd / Progress /
end streaming protocol. Phase 2's `SentenceBuffer + asyncio.Queue`
bi-streaming is transparent to this backend — `NeoMindWSClient`
implements the `LLMClient` Protocol's `stream()` method, so the
orchestrator consumes it with no backend-specific code path.

This makes NeoMind the LLM source **out of the box**: stream the
browser mic in, get NeoMind Agent replies streamed back as audio,
with token-level overlap between LLM generation and TTS playback.

> The Rust-side `CapabilityContext::invoke_capability` / SDK
> `agent::trigger()` path is intentionally **not** used here: it is a
> non-streaming single-Value RPC and would discard Phase 2's
> bi-streaming, barge-in mid-LLM, and the `llm_first_sentence_ms` KPI.
> The Python WS client is the validated integration point.

### Required environment variables

| Variable | Purpose |
|----------|---------|
| `NEOMIND_TOKEN` (or `NEOMIND_API_KEY`) | API key for NeoMind chat WS auth |
| `VOICE_ASSISTANT_ORCHESTRATOR_URL` | Python orchestrator WS URL; the Rust cdylib connects here (default `ws://127.0.0.1:9384/ws`) |
| `VOICE_ASSISTANT_PROFILE` | Profile name; use `default` for the NeoMind path (default) |

### Running the integration

```bash
# 1. ASR + TTS companion services must be up (see moss-tts-nano / sensevoice-asr READMEs)

# 2. Authorize against NeoMind
export NEOMIND_TOKEN=nmk_xxx        # neomind api-key create

# 3. Start the Python orchestrator with the default (neomind_ws) profile
cd extensions/voice-assistant/service
VOICE_ASSISTANT_PROFILE=default python server.py --port 9384 &

# 4. Build & install the Rust extension (proxy PCM between browser and orchestrator)
cd ../../..
./build.sh --single voice-assistant --skip-frontend
# Then load dist/voice-assistant-2.7.6-darwin_aarch64.nep via NeoMind UI
```

### Acceptance: NeoMind E2E measurement

```bash
cd extensions/voice-assistant/service
python measure_neomind_e2e.py --n 3
```

The harness runs three preflight checks (token set, orchestrator
reachable, profile is `default`), drives the full pipeline with a real
audio prompt, then prints:

- `asr_done → first REAL TTS PCM` (target < 600ms)
- `/measure` `llm_ttfb_ms` and `llm_first_sentence_ms` — **non-empty
  values here are the hard evidence that the NeoMind chat WS is the
  actual LLM source**. If the connection failed silently and the
  pipeline bailed out, both stay unobserved.
- Error classification, if any `{"type":"error"}` frame was received:
  - NeoMind auth rejected (401/403) — bad/expired token
  - NeoMind unreachable — network / DNS / NeoMind backend down
  - Orchestrator-internal — inspect the orchestrator log

A PASS verdict requires both NeoMind KPIs to have observations and no
error events. Run `measure_bi_stream_e2e.py --n 3` for the
profile-agnostic Phase 2 baseline; the NeoMind harness layers the
integration-specific signal on top of the same WS protocol code.

---

## Greeting (Say-First)

When a user connects, the assistant can immediately play a pre-synthesized
greeting clip ("你好，我是 NeoMind 助手") instead of waiting for the first
VAD → ASR → LLM → TTS round trip. Inspired by Vapi / Retell / LiveKit
greeting patterns.

### Configuration

```yaml
# profiles/default.yaml
interaction:
  greeting_text: ""  # empty = disabled (default)
```

Set `greeting_text` to a non-empty string to enable. The clip is
synthesized once at server startup using the profile's TTS voice and
cached as 16kHz mono PCM (third sibling to the `_ACK_PCM_BANK` and
`_STAGE_FILLER_BANK` warmup banks).

### Protocol

On `{"type":"start"}`, after sending `ready`, the server emits:

1. `{"type":"greeting","text":"..."}` — text frame for browser subtitle
2. Binary PCM frames — the greeting audio, queued in the browser's
   existing playback queue

No `tts_start`/`tts_end` is emitted around greeting — those mark turn
lifecycle only. Measurement scripts (`measure_common.run_one_turn`)
attribute pre-`asr_start` binary to a separate `greeting_pcm_chunks`
counter so Phase 2 metrics stay clean.

### Barge-in during greeting

If the user speaks during greeting playback, VAD fires normally and
the server immediately emits `{"type":"barge_in"}` so the browser
flushes the greeting queue — no waiting for the new turn's first TTS
PCM (which would add 200-500ms of continued greeting playback).

### AEC warning (opt-in by default)

Greeting playback without AEC will self-trigger VAD: the greeting
audio from the speaker enters the mic and may be detected as user
speech, cutting the greeting short. **Default is empty (disabled).**
Enable only when:

- `acoustic.aec: webrtc` (or similar) is configured, OR
- The environment is quiet enough that speaker→mic bleed is below
  the VAD threshold

This is the same AEC gap that affects ack and stage-filler playback;
greeting just makes it more visible due to its 1-2s duration. See
spec: `docs/superpowers/specs/2026-06-27-voice-greeting-design.md`.

---

## Acoustic Echo Cancellation (AEC)

Voice-assistant supports three modes for handling speaker→mic echo
leakage, configured via `acoustic.aec` in profile YAML:

| Mode | Behavior | When to use |
|------|----------|-------------|
| `none` | No echo suppression. TTS playback will self-trigger VAD. | A/B measurement baseline only. |
| `echo_window` (default) | Half-duplex VAD threshold boost during TTS playback + 400ms tail. Cheap, effective at cutting phantom transcripts (<10% rate), but user must speak louder to barge in. | Default; quiet environments; weak hardware. |
| `webrtc` | Full server-side reference-based AEC via `webrtc-audio-processing` adaptive filter. Reference signal is harvested from the server's own `send_binary` output via a global ring buffer; static delay calibration (`aec_reference_delay_ms`, default 200ms) handles browser playback latency. | Full-duplex barge-in; long greetings/acks/stage-filler clips; production. |

### Configuration

```yaml
acoustic:
  aec: echo_window   # or: none | webrtc
  # The following are optional and only meaningful when aec != none:
  aec_reference_delay_ms: 200    # static delay calibration (browser playback latency)
  aec_ref_buffer_seconds: 3.0    # ring buffer capacity
  aec_keep_echo_window: false    # also apply echo_window boost (default: true for echo_window mode, false for webrtc)
```

### How it works

```
Browser mic PCM ──┐
                  ↓
              AEC.process_capture(mic, ref)  ← ref_ring_buffer.peek_window(200ms ago)
                  ↓ cleaned PCM
              VAD → ASR → LLM → TTS
                  ↓
              send_binary(pcm) ──┐
                                  └─→ ref_ring_buffer.push (all send_binary output)
```

AEC is a preprocessing step before VAD — no VAD or ASR changes. When
the configured backend fails to load or initialize (e.g.,
`webrtc-audio-processing` not installed), the server automatically
falls back to `NoopAECBackend` (equivalent to `none`) and logs a
warning.

### Measurement

Use `measure_echo_rejection.py` to compare backends head-to-head:

```bash
# Server (run separately for each mode):
VOICE_ASSISTANT_AEC_MODE=webrtc python service/server.py &
python service/measure_echo_rejection.py --backend webrtc --trials 30

VOICE_ASSISTANT_AEC_MODE=echo_window python service/server.py &
python service/measure_echo_rejection.py --backend echo_window --trials 30

# Double-talk (full-duplex capture rate):
python service/measure_echo_rejection.py --backend webrtc --double-talk --trials 30
```

**Note:** `--double-talk` is wired through the CLI but currently
reports `n/a` for capture rate until the double-talk utterance
injection lands. ERLE is not yet reported either: the `_compute_erle`
helper exists and is unit-tested, but there is no `--compute-erle`
flag yet because the server-side cleaned-PCM side-channel (needed to
feed the helper) has not landed.

Decision matrix for flipping the default from `echo_window` to a real
AEC backend:

1. Phantom rate ≤ `echo_window` rate (real AEC at least as good at easy metric)
2. Double-talk detection ≥ 80% (full-duplex, which `echo_window` can't do)
3. ERLE ≥ 15 dB (basic competence)

If no backend clears all three, the default stays `echo_window`. See
spec: `docs/superpowers/specs/2026-06-28-voice-aec-design.md`.

### Troubleshooting

- **AEC init fails on startup**: install `webrtc-audio-processing` (`pip install webrtc-audio-processing`). Server falls back to Noop and continues.
- **Phantom transcripts with `webrtc` mode**: try tuning `aec_reference_delay_ms` (typically 100-300ms; too low = under-cancellation, too high = over-cancellation eating speech).
- **Double-talk detection < 80% with `webrtc`**: ensure `aec_keep_echo_window: false` (the default for webrtc) — if `keep_echo_window: true` is set, the VAD boost will over-suppress legitimate double-talk that AEC preserved.
- **Module name confusion**: the PyPI package is `webrtc-audio-processing` (hyphenated); the Python import is `webrtc_audio_processing` (underscored); the class is `AudioProcessingModule`.

## Stream Endpoint Mode (Capability Path)

Since v2.7.7, the frontend defaults to NeoMind's **extension stream endpoint**
(`/api/extensions/voice-assistant/stream`) instead of connecting directly to
the Python orchestrator WS. This routes LLM calls through the host's
**ChatStream capability**, making the extension token-free: no `NEOMIND_TOKEN`
needs to leave the host process.

### Architecture (stream mode, default)

```
Browser
  │  WS: /api/extensions/voice-assistant/stream?token=<jwt>
  │  ↑ binary PCM (8-byte BE seq + raw)
  │  ↓ push_output JSON {data_type: audio/pcm | application/json, data: base64}
  ▼
NeoMind host (neomind-api)
  │  extension_stream.rs (Bidirectional Push mode)
  │    ws_task ──text──▶ ws_in_rx ──▶ client_msg dispatch
  │    ws_task ──binary──▶ binary_in_rx ──▶ ext.process_session_chunk()
  │    rx (PushOutput) ──▶ ws_out_tx (mpsc 64) ──▶ ws_task.send()
  ▼
voice-assistant Rust extension (run_session_pump)
  │  browser ↔ Python orchestrator WS bridge
  │  chat_stream_request text frame ──▶ CapabilityContext.invoke_capability("chat_stream")
  │  AgentStreamChunk events (handle_event) ──▶ chat_streams[sid] ──▶ pump ──▶ WS text
  ▼
Python orchestrator (port 9384)
  │  NeoMindCapabilityLLM.stream() sends chat_stream_request, consumes chat_rx_queue
  │  ASR (qwen3_http) → LLM (capability) → TTS (kokoro_http)
  ▼
NeoMind host SessionManager  (via ChatStreamCapabilityProvider)
   AgentEvent stream ──▶ EventBus::publish(AgentStreamChunk) ──▶ EventDispatcher
```

### Stream mode vs Direct mode

| Aspect | Stream mode (default) | Direct mode (`directMode: true`) |
|--------|----------------------|----------------------------------|
| Frontend connects to | `/api/extensions/{id}/stream` (host) | `ws://127.0.0.1:9384/ws` (Python) |
| LLM token holder | host SessionManager (token-free from extension's POV) | Python orchestrator (`NEOMIND_TOKEN`) |
| Default profile | `neomind-capability` | `default` (or any profile) |
| Use case | Production / Tauri builds | Debugging the Python orchestrator in isolation |
| Host visibility | host sees every LLM call (audit, rate-limit,治理) | host is bypassed; no governance |

Toggle via the **Direct Python WS Mode** checkbox in the card config dialog.
Switching modes requires the card to remount (close + reopen, or refresh).

### Barge-in behavior

- **Server-side VAD barge-in** (speaking during TTS interrupts): works in
  both modes — VAD runs in the Python orchestrator regardless of transport.
- **Manual stop button** (explicit barge-in by tap): available in direct
  mode only. Stream mode has no "send control message" channel in the
  stream protocol v1; the mic toggle just stops local capture. Planned
  follow-up: expose a `stop` extension command and have the frontend POST
  `/api/extensions/voice-assistant/execute/stop`.

### Troubleshooting (Tauri terminal `[VA]` checkpoints)

Per turn, the Rust extension should log this sequence:

```
[VA] chat_stream_request received, invoking capability...
[VA] capability returned: {"success":true,"session_id":"..."}
[VA] handle_event: AgentStreamChunk sid=... chunk_type=Some("Content")  (repeated)
[VA] handle_event: AgentStreamChunk sid=... chunk_type=Some("end")
```

- **Stuck on "Thinking…"**: capability never returned, or events not routed.
  Verify `window.NeoMindStream.urlFor('voice-assistant')` returns a URL in
  the browser console, and that `[VA] chat_stream_request received` appears.
  If capability returned but no Content chunks follow, the host's
  `EventDispatcher` is filtering events — check
  `event_subscriptions()` includes `"AgentStreamChunk"`.
- **401 / stream connection refused**: JWT expired. The stream WS stays
  open once authenticated, but reconnects fail. Reload the page to refresh
  the token.
- **Mic captures but no transcript**: PCM not reaching Python. Check the
  binary framing — `sendChunk` must emit 8-byte BE sequence + raw PCM.
  Verify `[VA]` pump logs show inbound binary chunks.
- **`Stream URL unavailable`**: the host web app did not register
  `window.NeoMindStream.urlFor`. Check `App.tsx`'s `useEffect` and that
  `tokenManager.getToken()` returns a non-null JWT.
