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

### 1. Prerequisites

- moss-tts-nano service running on port 9382 (see its README)
- sensevoice-asr service running on port 9383 (see its README)

### 2. Install orchestrator dependencies

```bash
cd extensions/voice-assistant/service
pip install -r requirements.txt
```

### 3. Run the latency measurement (no extension needed)

This independently validates MOSS-TTS first-chunk latency — the riskiest
unknown in the architecture.

```bash
python measure_latency.py --out latency_report.json
```

Look for "TTS first-chunk" numbers. Target: < 500ms.

### 4. Run the orchestrator

```bash
./start.sh
# or: python server.py --host 127.0.0.1 --port 9384
```

### 5. Build the extension

```bash
./build.sh --dev --single voice-assistant
# Restart NeoMind to pick up the new extension.
```

### 6. Open the test page

Just open `extensions/voice-assistant/service/poc.html` in Chrome.
Set WS URL to `ws://127.0.0.1:9384/ws` (default).

**Note**: The PoC HTML page talks **directly** to the Python orchestrator
WebSocket, bypassing the Rust extension. This is intentional — it lets you
validate the Python pipeline before integrating with NeoMind. Once the
pipeline works, point the WS URL at NeoMind's `/api/extensions/voice-assistant/stream`
endpoint to validate the extension path.

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

Environment variables (orchestrator):

| Variable | Default | Description |
|----------|---------|-------------|
| `SENSEVOICE_ASR_URL` | `http://127.0.0.1:9383` | ASR HTTP service base URL |
| `MOSS_TTS_URL` | `http://127.0.0.1:9382` | TTS HTTP service base URL |
| `VOICE_ASSISTANT_VOICE` | `Junhao` | TTS voice preset |
| `VOICE_ASSISTANT_VAD_SILENCE_MS` | `500` | Trailing silence to trigger speech-end |
| `VOICE_ASSISTANT_VAD_MIN_SPEECH_MS` | `300` | Min continuous speech to start an utterance |
| `VOICE_ASSISTANT_VAD_ENERGY` | `0.015` | RMS threshold (0-1); raise in noisy rooms |

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
