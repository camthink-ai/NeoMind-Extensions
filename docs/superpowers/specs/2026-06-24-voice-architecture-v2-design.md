# Voice Assistant v2 Architecture Design

**Date:** 2026-06-24
**Status:** Design (awaiting implementation plan)
**Predecessors:** voice-edge-stack plan (2026-05-09), Silero VAD integration (Part B, completed)

## Context

The current voice-assistant (Python FastAPI service at port 9384) achieved end-to-end functionality with ~3s total latency using Silero VAD + SenseVoice ASR + Ollama + ZipVoice TTS. The pipeline works but:

- The orchestrator (`server.py`, 1174 lines) mixes FSM logic, backend calls, and HTTP routing in one file
- Turn state is tracked via implicit epoch counter + scattered booleans (race-prone)
- Barge-in is unidirectional (TTS-stop only, no LLM cancel)
- Backend selection is hardcoded if/elif dispatch — no pluggability
- AEC is absent (open-mic scenarios will fail self-barge-in)
- No latency instrumentation beyond scattered `time.perf_counter()`

This spec defines a refactor + capability expansion to:
1. Enable pluggable backends for 4 scenarios (hardware, acoustic, language, interaction)
2. Implement full barge-in (TTS cancel + LLM `__CANCEL__` + queue clear)
3. Compress first-audio latency from ~1s toward 500ms via pipeline overlap
4. Integrate with NeoMind chat API (including tool-call TTS pollution mitigation)
5. Provide telemetry proving latency targets

## Non-Goals (Phase 1)

- Bi-directional token-level streaming (Phase 2 research)
- Server-side AEC via speexdsp (Phase 2)
- Native Rust extension (Phase 3 option)
- Streaming ASR / streaming TTS replacements (Phase 2 research)
- Deleting moss-tts-nano / cosyvoice-3 extensions (kept as fallback)

## Architecture Decision

**Architecture C: Orchestrator + typed contracts + pluggable backends via profile YAML.**

The orchestrator depends only on Python `Protocol` interfaces (contracts). Backends are concrete classes implementing those Protocols, instantiated by factory functions reading from a `Profile`. Swapping a backend = new YAML entry + one factory line, zero orchestrator changes.

### Integration Phases

| Phase | Scope | Target |
|-------|-------|--------|
| **Phase 1** | Python orchestrator refactor + explicit FSM + full barge-in + NeoMind chat + telemetry | <1s first audio, usable, pluggable |
| **Phase 2** | speexdsp AEC + streaming ASR research + streaming TTS research + NeoMind React card | <500ms first audio, production |
| **Phase 3 (optional)** | Rust native extension orchestrator | Edge deployment, <100MB memory |

**Backend implementations stay Python across all phases.** Phase 3 only rewrites the orchestrator; backend services are reused unchanged. This is the value of contract-based decoupling.

## NeoMind Extension System Compatibility Assessment

Confirmed (SDK source evidence):

- ✅ `StreamDataType::Audio { format, sample_rate, channels }` — audio is first-class (`neomind-extension-sdk/src/host.rs:406`)
- ✅ `send_push_output(PushOutputMessage)` — real-time push to frontend (`host.rs:1065`)
- ✅ `StreamDirection::Duplex` — full-duplex stream (mic in / audio out)
- ✅ `tokio::runtime::Handle::spawn()` — long-lived background tasks (homeassistant-bridge pattern)
- ⚠️ Commands are serialized in runner main loop (`neomind-extension-runner/src/main.rs:2376`) — voice sessions MUST be background tasks, not long-running commands
- ❌ SDK provides no WebSocket/HTTP client abstraction — use `tokio-tungstenite` / `ureq` directly
- ❌ NeoMind web has no mic/audio infrastructure — build from scratch via WebAudio API

**Conclusion:** NeoMind core + SDK + extension system fully supports this plan. Voice session runs as background task spawned at `configure()`. Commands only do lightweight start/stop/status/measure. Real-time audio path is browser ↔ Python service (direct WS), bypassing NeoMind stream system to minimize latency.

## Section 1: Component Boundaries & Typed Contracts

All backend interfaces defined as Python `Protocol` (structural typing, no inheritance coupling). All files use `from __future__ import annotations` to enable modern type hints (`list[float]`, `str | None`) on Python 3.9+.

```python
# contracts.py — single source of truth for backend interfaces
from __future__ import annotations
from dataclasses import dataclass
from typing import Protocol, AsyncIterator

@dataclass
class VadSegment:
    samples: list[float]      # 16kHz mono float32
    sample_rate: int
    start_ms: int
    end_ms: int

class VADBackend(Protocol):
    def feed(self, pcm_int16: bytes) -> list[VadSegment]: ...
    def flush(self) -> list[VadSegment]: ...
    @property
    def sample_rate(self) -> int: ...

@dataclass
class PartialTranscript:
    text: str
    is_final: bool
    confidence: float

class ASRBackend(Protocol):
    async def transcribe(self, pcm_float32: list[float], sample_rate: int) -> str: ...
    async def stream(self, pcm_iterator) -> AsyncIterator[PartialTranscript]:
        raise NotImplementedError  # Phase 2 path

@dataclass
class LlmEvent:
    # Type values match NeoMind WS event casing (verified in NeoMind source):
    # crates/neomind-api/src/handlers/sessions.rs serializes End as lowercase "end",
    # others are PascalCase. Error casing varies — handle both.
    type: str  # "Content" | "Thinking" | "ToolCallStart" | "ToolCallEnd" | "Progress" | "end" | "Error" | "error"
    text: str | None = None
    tool_name: str | None = None
    progress: float | None = None  # 0.0-1.0 for Progress events

class LLMClient(Protocol):
    async def stream(self, user_text: str, session_id: str) -> AsyncIterator[LlmEvent]: ...
    async def cancel(self, session_id: str) -> None: ...

@dataclass
class TtsChunk:
    pcm_int16: bytes
    sample_rate: int
    is_final: bool

class TTSBackend(Protocol):
    async def synthesize(self, text: str, voice: str) -> bytes: ...
    async def stream(self, text: str, voice: str) -> AsyncIterator[TtsChunk]:
        raise NotImplementedError  # Phase 2 path

class AECBackend(Protocol):
    def process(self, mic_pcm: bytes, reference_pcm: bytes) -> bytes: ...
```

**Why Protocol not ABC:** Duck typing. Adding a backend = new file, no edit to existing classes. Test mocks trivially satisfy Protocol shape without inheritance.

Profile YAML selects which implementation:

```yaml
# profiles/default.yaml
acoustic:
  vad_backend: silero
  vad_threshold: 0.5
  aec: none
backends:
  asr:
    type: sensevoice_http
    url: http://127.0.0.1:9383
  llm:
    type: neomind_ws
    url: ws://127.0.0.1:9375/api/chat
    voice_mode: true   # route only Content events to TTS
  tts:
    type: zipvoice_http
    url: http://127.0.0.1:9386
    voice: 中文女
```

### Pending decisions (recommended, awaiting confirmation)

| Decision | Recommendation | Rationale |
|----------|----------------|-----------|
| VAD in-proc vs out-of-proc | **in-proc** | VAD called every 32ms; HTTP round-trip 5-10ms is unacceptable |
| ASR/TTS HTTP vs WS | **Phase 1: HTTP; Phase 2+: WS** | HTTP simpler, already working. WS only needed for streaming backends |
| Profile storage | **YAML** | <20 profiles, no transactions, ops-editable, version-controllable |

## Section 2: Explicit State Machine + Pipeline Parallelism

### 2.1 Five-state FSM (replaces implicit epoch counter)

```
IDLE ─[VAD speech-start]─► LISTENING
  ▲                            │
  │                            │ [VAD speech-end + final transcript]
  │                            ▼
  │                         THINKING
  │                            │ [first TTS chunk OR first LLM token]
  │                            ▼
  │                         SPEAKING
  │                            │ [TTS done OR barge-in]
  └────────────────────────────┘
  │
  └─[barge-in detected]─ BARGED ─[cancel complete]─► LISTENING
```

Every transition logged and testable. Impossible state combinations eliminated.

| From → To | Trigger | Actions |
|-----------|---------|---------|
| IDLE → LISTENING | VAD speech-start | Open ASR session |
| LISTENING → THINKING | VAD speech-end + final transcript | Send LLM request; cancel any in-flight TTS (user self-interruption) |
| THINKING → SPEAKING | First TTS chunk ready | Stream PCM to browser |
| SPEAKING → IDLE | TTS playback complete | Cleanup; reopen mic |
| ANY → BARGED | User speech during THINKING/SPEAKING | 4 parallel cleanup actions (see Section 3) |
| BARGED → LISTENING | Cleanup complete + new turn | Start fresh ASR session |

### 2.2 Pipeline overlap (latency budget)

**Current (~1.7s sequential):**
```
t=0     VAD end
t=200ms ASR complete
t=1130ms LLM TTFB
t=1700ms First TTS chunk to speaker
```

**Target (overlap):**
- VAD-stream → ASR-partial (streaming ASR starts before VAD-end) — Phase 2
- ASR-final → LLM-stream (start on partial, refine on final) — Phase 2
- LLM-token → TTS-chunk (true streaming TTS emits at punctuation) — Phase 2

**Phase 1 targets (current backends, no streaming):**

| Metric | Phase 1 | Phase 2 |
|--------|---------|---------|
| First audio out | ~900ms | <500ms |
| Full turn (30 char) | ~2.5s | ~1.2s |
| Barge-in latency | <200ms | <100ms |

**Honest constraint:** SenseVoice is batch ASR (300ms hard floor). ZipVoice is pseudo-streaming. Phase 1 cannot reach 500ms without swapping these backends. Architecture must not hardcode their limitations — `stream()` methods exist on Protocols, invoked when backends implement them.

## Section 3: Barge-in Protocol + NeoMind `__CANCEL__` Integration

### 3.1 Barge-in detection

**Phase 1 path (ASR-based):**
- Mic stays open during SPEAKING
- VAD continues running with `threshold + 0.2` (reduce TTS feedback false-positives)
- Secondary check: VAD segment duration > 300ms (filter coughs/keyboard)
- Documented as workaround until Phase 2 speexdsp AEC

**Phase 2 path (browser AEC):**
- `getUserMedia({ echoCancellation: true })` removes TTS echo before VAD
- Threshold hack removed once AEC verified

### 3.2 Four parallel cleanup actions on barge-in

```python
async def _handle_barge_in(self, reason: str):
    prev_state = self.state
    self.transition(State.BARGED)  # blocks all new actions

    cleanup_tasks = [
        self._cancel_tts_playback(),      # ① Browser stops playback via WS control frame
        self._cancel_llm_request(),        # ② Send NeoMind __CANCEL__
        self._clear_pending_queues(),      # ③ Drain TTS/PCM/sentence buffers
        self._drain_asr_buffer(),          # ④ Preserve new speech, restart ASR session
    ]
    results = await asyncio.gather(*cleanup_tasks, return_exceptions=True)
    for r in results:
        if isinstance(r, Exception):
            logger.warning("barge-in cleanup task failed: %s", r)

    self.transition(State.LISTENING)
    self._asr_session = await self._asr.start_session()
```

### 3.3 NeoMind `__CANCEL__` integration details

**Verified protocol** (from NeoMind source `crates/neomind-agent/src/agent/streaming/stream_core.rs:546-547` and `crates/neomind-api/src/handlers/sessions.rs:222-224`):

When `__CANCEL__` is received, NeoMind emits:
1. `{"type": "Content", "content": "\n\n[Interrupted]"}` (interrupt marker)
2. `{"type": "end", "sessionId": "..."}` (lowercase "end", no reason field)

```python
async def _cancel_llm_request(self):
    if self._llm_completed:   # already got End event, no-op
        return
    if self._neomind_ws and not self._neomind_ws.closed:
        await self._neomind_ws.send(json.dumps({"type": "__CANCEL__"}))
        try:
            # Wait for lowercase "end" event (NOT "End" with reason field)
            await asyncio.wait_for(self._wait_for_end_event(), timeout=0.5)
        except asyncio.TimeoutError:
            logger.warning("NeoMind cancel ack timeout, reconnecting")
            await self._reconnect_neomind_ws()

async def _wait_for_end_event(self):
    async for msg in self._neomind_ws:
        evt = json.loads(msg)
        if evt.get("type") == "end":   # lowercase, verified
            return
        if evt.get("type") == "Content":
            # Discard the "[Interrupted]" marker — do NOT feed to TTS
            continue
```

**Critical:**
- `__CANCEL__` sent on same WS connection as the LLM stream (not a new connection)
- NeoMind emits a `Content` event with `"\n\n[Interrupted]"` text BEFORE the `end` event — this MUST be filtered out, not sent to TTS
- The terminating event is lowercase `"end"` (NOT `"End"` with a reason field)
- If LLM already completed (TTS still playing), `__CANCEL__` is no-op (idempotent)
- 500ms timeout → force reconnect

### 3.4 TTS pollution mitigation (NeoMind chat events)

NeoMind chat emits 10+ event types. Only `Content` events feed TTS. Others route to UI state indicator:

| Event | TTS action | UI action |
|-------|-----------|-----------|
| `Content` | ✅ feed to TTS (unless text == "\n\n[Interrupted]") | — |
| `Thinking` | ❌ skip | "正在思考..." indicator |
| `ToolCallStart` | ❌ skip | earcon + "正在查询 {tool_name}..." |
| `ToolCallEnd` | ❌ skip | remove indicator |
| `Progress` | ❌ skip | progress bar |
| `end` (lowercase) | finalize turn | clear indicators |
| `Error` / `error` | speak error message | error banner |

**Special case:** After `__CANCEL__`, NeoMind emits `Content` with text `"\n\n[Interrupted]"`. This MUST be filtered out — do not send to TTS (user would hear "interrupted" spoken aloud).

### 3.5 ASR session lifecycle & barge-in semantics

**ASRBackend Protocol is stateless:** `transcribe(pcm, sample_rate) -> str` — no session handle needed. The orchestrator manages ASR state:

- **IDLE → LISTENING:** No ASR session opened yet. VAD accumulates audio.
- **VAD speech-end:** Orchestrator passes accumulated segment to `asr.transcribe()`. Single call, returns final string.
- **Barge-in:** If currently in `asr.transcribe()` call (THINKING state), the in-flight HTTP request is cancelled via `httpx.AsyncClient.aclose()` or task cancellation. The partial transcript is **discarded** — barge-in means user changed intent, prior input is stale.

**Barge-in preserves the NEW user speech** (post-barge-in audio captured by VAD), not the OLD partial ASR. The new VAD segment becomes input to the next `asr.transcribe()` call.

```python
async def _drain_asr_buffer(self):
    # Cancel any in-flight ASR HTTP request
    if self._asr_task and not self._asr_task.done():
        self._asr_task.cancel()
    # The current VAD segment (user's new speech) is PRESERVED
    # in self._vad_buffer — it becomes input to next turn
    # Old partial transcripts are discarded
```

### 3.6 Concurrency safety

Barge-in is race-prone. Required invariants:
- `asyncio.Lock` around state transitions
- Idempotent cleanup (multiple barge-ins within 200ms execute cleanup once)
- `test_concurrent_barge_in_idempotent` must pass

## Section 4: Pluggability / Profile System

### 4.1 Profile YAML examples

```yaml
# profiles/default.yaml — developer default
name: default
description: "Developer default: M4 Pro + desktop mic + CN/EN"
hardware:
  cpu_threads: 4
acoustic:
  vad_backend: silero
  vad_threshold: 0.5
  vad_silence_ms: 500
  aec: none
backends:
  asr: { type: sensevoice_http, url: http://127.0.0.1:9383 }
  llm: { type: neomind_ws, url: ws://127.0.0.1:9375/api/chat, voice_mode: true }
  tts: { type: zipvoice_http, url: http://127.0.0.1:9386, voice: 中文女 }
interaction:
  barge_in: full
  latency_target_ms: 1000
```

```yaml
# profiles/edge-arm.yaml — RK3588 / Jetson
hardware:
  cpu_threads: 6
acoustic:
  aec: speexdsp           # open-mic requires AEC
backends:
  llm:
    type: ollama_http     # edge can't reach cloud LLM
    url: http://127.0.0.1:11434/api/chat
    model: qwen3:1.7b
interaction:
  latency_target_ms: 1500  # CPU slower, relax budget
```

```yaml
# profiles/noisy-env.yaml — factory / vehicle
acoustic:
  vad_threshold: 0.7       # raise to reduce false-triggers
  vad_min_speech_ms: 500
  noise_suppression: rnnoise
  aec: speexdsp
```

```yaml
# profiles/headset.yaml — near-field, no echo path
acoustic:
  vad_backend: energy      # simple energy gate sufficient
  vad_threshold: 0.3
  aec: none                # headset: no echo path
```

### 4.2 Loading & overrides

Priority chain: **env var > named profile > default**. Env vars enable ad-hoc dev tweaks ("let me lower threshold to test") without creating new YAML files.

### 4.3 Backend factory

```python
def make_vad(profile):
    match profile.vad_backend_type:
        case "silero": return SileroVAD(**profile.vad_config)
        case "energy": return EnergyVAD(**profile.vad_config)

def make_asr(profile): ...
def make_llm(profile): ...
def make_tts(profile): ...
```

**Adding a backend:** new file implementing Protocol + one `case` line in factory + new YAML profile. Zero orchestrator changes.

### 4.4 Phase mapping

| Protocol | Phase 1 (existing) | Phase 2 (research) |
|----------|-------------------|--------------------|
| VADBackend | SileroVAD, EnergyVAD | FsmnVAD, WebRTC VAD |
| ASRBackend | SenseVoiceHTTPASR (batch) | WhisperStreamingASR, ParaformerStreaming |
| LLMClient | NeoMindWSClient, OllamaHTTPClient | (no change) |
| TTSBackend | ZipVoiceHTTP (pseudo-stream) | CosyVoiceStreaming, KokoroBatch |

## Section 5: AEC Strategy

### 5.1 Three layers (by ROI)

**Layer 1: Browser EchoCancellation (always on, free)**
- `getUserMedia({ echoCancellation: true, noiseSuppression: true, autoGainControl: true })`
- WebRTC AEC3, tuned for telephony
- Always enabled, zero config
- **Caveat:** Effectiveness for TTS echo (especially music/long phrases) is untested in this project. May fail with >50ms speaker-mic distance or loud speaker volume. Headset scenarios are reliable; open-speaker scenarios should expect occasional false barge-ins until Phase 2 speexdsp lands.

**Layer 2: Server-side speexdsp (Phase 2)**
- Orchestrator maintains reference PCM ring buffer (last 500ms of TTS output)
- `AECBackend.process(mic_pcm, reference_pcm) → cleaned_pcm`
- Applied before VAD when `acoustic.aec: speexdsp` in profile
- Python bindings mature enough; ~50ms alignment tuning required

**Layer 3: ALSA/HAL AEC (Phase 3, edge deployment)**
- PulseAudio/PipeWire with aec plugin, or RNNoise+speexdsp in ALSA chain
- Platform-specific, not cross-platform

### 5.2 Phase 1 decision

Use **Layer 1 only + threshold hack**. Documented limitation: open-speaker scenarios will have occasional false-barge-ins. Headset scenarios work fine.

### 5.3 AEC is orchestrator-agnostic

```python
# In audio pipeline:
if self.aec is not None:
    mic_pcm = self.aec.process(mic_pcm, self._reference_pcm)
segments = self.vad.feed(mic_pcm)
```

Adding AEC = backend swap. Profile field defaults to `none`. No orchestrator logic change.

## Section 6: Telemetry / Metrics

### 6.1 Seven core KPIs

| KPI | Definition | Phase 1 | Phase 2 |
|-----|-----------|---------|---------|
| `asr_complete_ms` | VAD-end → ASR final | <250ms | <150ms |
| `llm_ttfb_ms` | ASR final → first Content event | <600ms | <300ms |
| `tts_first_chunk_ms` | first sentence complete → first PCM to browser | <300ms | <100ms |
| **`first_audio_out_ms`** | VAD-end → user hears first phoneme | **<1000ms** | **<500ms** |
| `full_turn_ms` | VAD-end → TTS playback end | <3000ms | <1500ms |
| `barge_in_to_silence_ms` | user speaks → speaker silent | <200ms | <100ms |
| `barge_in_cancel_ack_ms` | user speaks → NeoMind ack | <700ms | <400ms |

**`first_audio_out_ms` is the user-facing metric.** Sum of others minus pipeline overlap.

### 6.2 OpenTelemetry lightweight tracing

Console-only span output (no Jaeger/Tempo dependency for Phase 1):

```
voice.turn (first_audio_out_ms = 920ms)
├── asr.transcribe    (220ms)
├── llm.stream        (ttfb=590ms)
│   ├── llm.token[0]  (590ms)
│   └── llm.end       (1100ms)
├── tts.synthesize    (110ms)
└── tts.deliver[0]    (920ms from turn start)
```

Enabled via `VOICE_ASSISTANT_TRACE=1`. Off by default (log volume).

### 6.3 Rolling percentiles (always on)

```python
class RollingPercentile:
    def __init__(self, window=100): self.samples = deque(maxlen=window)
    def observe(self, v): self.samples.append(v)
    def percentile(self, p):
        s = sorted(self.samples); return s[min(int(len(s)*p/100), len(s)-1)]
```

100 samples × 7 KPIs = 5KB memory. Always in memory, reported via `produce_metrics` and `measure` command.

### 6.4 `measure` command

```python
case "measure":
    n = args.get("n", 5)
    results = await self._measure_latency(n)
    return {
        "first_audio_out_ms": {"p50": 920, "p95": 1180},
        "asr_complete_ms": {"p50": 210, "p95": 280},
        "llm_ttfb_ms": {"p50": 590, "p95": 720},
        "tts_first_chunk_ms": {"p50": 120, "p95": 180},
        "target_ms": 1000,
        "target_met": True
    }
```

Callable from NeoMind UI. No separate measurement script needed.

### 6.5 Phase 1 target honesty

Based on measured e2e latency:
- SenseVoice batch ASR: ~200ms
- Ollama LLM TTFB: ~600ms
- ZipVoice first chunk: ~150ms (RTF 0.3, ~500ms utterance)
- Browser WS round-trip: ~30ms
- **Sum: ~980ms. Phase 1 target revised to <1200ms** (was <1000ms) to leave 200ms jitter margin. The <1000ms figure remains the **stretch goal**, not commitment.

Phase 2 <500ms **requires** streaming ASR + streaming TTS. Pipeline overlap alone cannot save 500ms. This is the Phase 2 research item.

`interaction.latency_target_ms` in profile YAML is **informational** — used for telemetry warnings ("target exceeded") and `measure` command reporting. It does not change orchestrator behavior.

## Section 7: File / Module Structure

### 7.1 Target structure (Phase 1)

```
extensions/voice-assistant/
├── Cargo.toml
├── src/lib.rs                      # Rust proxy extension (minimal)
├── service/
│   ├── server.py                   # FastAPI routes (~200 lines, down from 1174)
│   ├── orchestrator.py             # FSM + pipeline (~400 lines)
│   ├── contracts.py                # All Protocols (~100 lines)
│   ├── profile.py                  # YAML load + env override (~80 lines)
│   ├── telemetry.py                # OTel + rolling percentile (~120 lines)
│   ├── stats.py                    # RollingPercentile class (~50 lines)
│   ├── ws_protocol.py              # Browser WS frame codec (~100 lines)
│   ├── start.sh
│   ├── requirements.txt
│   ├── profiles/
│   │   ├── default.yaml
│   │   ├── edge-arm.yaml
│   │   ├── noisy-env.yaml
│   │   └── headset.yaml
│   ├── backends/
│   │   ├── __init__.py             # Factory functions
│   │   ├── vad.py                  # SileroVAD, EnergyVAD (~150 lines)
│   │   ├── asr.py                  # SenseVoiceHTTPASR (~80 lines)
│   │   ├── llm.py                  # NeoMindWSClient, OllamaHTTPClient (~250 lines)
│   │   ├── tts.py                  # ZipVoiceHTTP (~80 lines)
│   │   └── aec.py                  # SpeexAEC (Phase 2, stub)
│   ├── assets/poc.html
│   └── tests/
│       ├── test_state_machine.py
│       ├── test_barge_in.py
│       ├── test_pipeline.py
│       ├── test_silero_vad.py      # existing, kept as-is
│       ├── test_profile_loading.py
│       └── test_contracts.py
```

**Total:** 12 Python files, avg ~140 lines, all <400 lines.

### 7.2 Dependency graph (no cycles)

```
contracts.py (stdlib only)
    ↑
backends/{vad,asr,llm,tts,aec}.py (depend on contracts)
    ↑
profile.py (reads YAML, returns Profile)
    ↑
orchestrator.py (depends on Protocols only)
    ↑
server.py + telemetry.py (FastAPI glue)
```

Adding a backend = new leaf file + one factory line. Zero orchestrator edit.

### 7.3 Migration strategy (incremental, e2e verified)

**Rule: after each step, e2e pipeline must still work.**

1. Create `contracts.py` (extract Protocols; server.py unchanged)
2. Create `profile.py` + `profiles/default.yaml` (extract env defaults)
3. Create `backends/vad.py` (move SileroVAD/EnergyVAD; verify e2e)
4. Create `backends/{asr,tts,llm}.py` (extract HTTP/WS clients; verify each)
5. Create `orchestrator.py` (extract turn logic + add FSM + barge-in) — highest risk step
6. Add `telemetry.py`, `stats.py` (wire OTel + percentiles)
7. Slim `server.py` to ~200 lines (routes + orchestrator instantiation only)

Each step independently committable. Rollback on e2e break.

### 7.4 Phase 2: NeoMind React card

```
extensions/voice-assistant/frontend/
├── frontend.json
├── src/
│   ├── index.tsx              # VoiceAssistantCard
│   ├── audio-engine.ts        # mic capture + AudioWorklet
│   ├── pcm-player.ts          # PCM playback
│   ├── ws-client.ts           # WS to Python service (same Phase 1 protocol)
│   └── state-indicator.tsx    # IDLE/LISTENING/THINKING/SPEAKING badge
```

Card connects directly to `ws://127.0.0.1:9384` (Python service). NeoMind stream system NOT used for audio (latency). NeoMind sees only `execute_command` for start/stop/status/measure.

### 7.5 Phase 3 (optional): Rust native orchestrator

```
extensions/voice-assistant-rs/
├── src/
│   ├── lib.rs                 # NeoMind extension + stream session
│   ├── orchestrator.rs        # ported from orchestrator.py
│   ├── contracts.rs           # traits from contracts.py
│   ├── state_machine.rs
│   └── backends/
│       ├── vad.rs             # HTTP to Python Silero service
│       ├── asr.rs             # HTTP to SenseVoice
│       ├── llm.rs             # WS to NeoMind
│       └── tts.rs             # HTTP to ZipVoice
```

**Backend services stay Python.** Only orchestrator rewritten in Rust. Eliminates ~30-50ms Python overhead + gains NeoMind native extension benefits (cdylib, no process overhead, edge deployment).

## Critical Files (Implementation Plan Will Reference)

| Path | Action |
|------|--------|
| `service/contracts.py` | Create — all Protocols |
| `service/profile.py` + `profiles/*.yaml` | Create — config system |
| `service/backends/{vad,asr,llm,tts,aec}.py` | Create — extract from server.py |
| `service/orchestrator.py` | Create — FSM + pipeline + barge-in |
| `service/telemetry.py` + `stats.py` | Create — instrumentation |
| `service/ws_protocol.py` | Create — frame codec |
| `service/server.py` | Slim from 1174 to ~200 lines |
| `service/tests/test_*.py` | Create — TDD per component |
| `src/lib.rs` | Unchanged (proxy pattern) |

## Risks & Mitigations

1. **SenseVoice batch ASR caps Phase 1 floor at ~300ms** — accepted; Phase 2 swaps to streaming
2. **NeoMind WS protocol instability** — wrap in `NeoMindWSClient`, isolate from orchestrator
3. **Barge-in race conditions** — TDD coverage of all transitions + concurrent barge-in test
4. **speexdsp Python bindings immaturity** — defer to Phase 2; Layer 1 browser AEC sufficient for dev
5. **Migration breaks e2e** — incremental 7-step migration, verify after each
6. **Profile YAML sprawl** — start with 4 canonical profiles; add as scenarios proven

## Success Criteria

| Criterion | Measurement |
|-----------|------------|
| `first_audio_out_ms` p50 < 1200ms (stretch: <1000ms) | `measure` command, n=10 turns |
| Barge-in → speaker silence < 200ms | test_barge_in.py + manual verification |
| Pluggable backends verified | Run with `headset.yaml` and `default.yaml`, both work |
| NeoMind chat TTS filter | **Objective:** TTS receives ZERO events of type Thinking/ToolCallStart/ToolCallEnd/Progress, and ZERO Content events with text "\n\n[Interrupted]". Verified by mock NeoMind WS emitting all event types. |
| All tests green | `pytest service/tests/` |
| orchestrator.py < 400 lines | file size check |
| server.py < 250 lines | file size check |

## Open Questions (for implementation plan)

- Streaming ASR candidate evaluation (Paraformer-streaming vs Whisper-streaming vs sherpa online)
- Streaming TTS candidate evaluation (CosyVoice streaming vs GLM-TTS streaming)
- speexdsp Python binding selection (py-esps vs python-speex vs ctypes wrapper)
- NeoMind WS reconnect strategy under network jitter
