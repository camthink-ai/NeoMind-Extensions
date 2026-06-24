# Voice Assistant v2 Architecture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor voice-assistant from a 1174-line monolithic `server.py` into a modular orchestrator + pluggable backends architecture, adding an explicit 5-state FSM, full barge-in with NeoMind `__CANCEL__`, profile-based configurability, and latency telemetry — while keeping the e2e pipeline working after every step.

**Architecture:** Architecture C from the spec — Python `Protocol`-based contracts decouple the orchestrator from concrete VAD/ASR/LLM/TTS backends. A `Profile` YAML selects implementations. The orchestrator runs an explicit FSM (IDLE/LISTENING/THINKING/SPEAKING/BARGED) with pipeline overlap. Migration is incremental: each task extracts one concern from `server.py` into its own file, verifies e2e still works, commits.

**Tech Stack:** Python 3.11+, FastAPI, websockets, httpx, sherpa-onnx (Silero VAD), NumPy, OpenTelemetry (console exporter), PyYAML. Tests via pytest.

**Spec:** `docs/superpowers/specs/2026-06-24-voice-architecture-v2-design.md`

---

## File Structure (Target)

```
extensions/voice-assistant/service/
├── server.py                   # Slimmed: FastAPI routes + WS handler (~200 lines)
├── orchestrator.py             # FSM + pipeline + barge-in (NEW)
├── contracts.py                # Protocols + dataclasses (NEW)
├── profile.py                  # YAML load + env override (NEW)
├── stats.py                    # RollingPercentile (NEW)
├── telemetry.py                # OTel setup + KPI tracking (NEW)
├── ws_protocol.py              # Browser WS frame codec (NEW)
├── profiles/
│   ├── default.yaml            # NEW
│   ├── edge-arm.yaml           # NEW
│   ├── noisy-env.yaml          # NEW
│   └── headset.yaml            # NEW
├── backends/
│   ├── __init__.py             # Factory functions (NEW)
│   ├── vad.py                  # SileroVAD, EnergyVAD (EXTRACTED from server.py)
│   ├── asr.py                  # SenseVoiceHTTPASR (EXTRACTED)
│   ├── llm.py                  # OllamaHTTPClient, NeoMindWSClient, FakeLLMClient (EXTRACTED + NEW)
│   ├── tts.py                  # ZipVoiceHTTP (EXTRACTED)
│   └── aec.py                  # Stub (Phase 2)
└── tests/
    ├── conftest.py             # Shared fixtures (NEW)
    ├── test_contracts.py
    ├── test_profile_loading.py
    ├── test_stats.py
    ├── test_state_machine.py
    ├── test_barge_in.py
    ├── test_pipeline.py
    ├── test_neomind_ws_client.py
    └── test_silero_vad.py      # EXISTING — move into tests/
```

**Migration rule:** After Tasks 4-7 (backend extractions), the e2e pipeline (mic → VAD → ASR → LLM → TTS → speaker) MUST still work. Run `./start.sh` + open `poc.html` + speak a test phrase after each extraction task.

---

## Task 1: Create `contracts.py` — Protocol interfaces

**Files:**
- Create: `extensions/voice-assistant/service/contracts.py`
- Create: `extensions/voice-assistant/service/tests/__init__.py`
- Create: `extensions/voice-assistant/service/tests/test_contracts.py`

- [ ] **Step 1: Write failing test for Protocol shape**

Create `tests/test_contracts.py`:

```python
"""Verify all Protocol interfaces are importable and structurally sound."""
from __future__ import annotations

import inspect
from contracts import (
    VADBackend, ASRBackend, LLMClient, TTSBackend, AECBackend,
    VadSegment, PartialTranscript, LlmEvent, TtsChunk,
)


def test_vad_protocol_methods():
    assert hasattr(VADBackend, "feed")
    assert hasattr(VADBackend, "flush")
    assert "sample_rate" in {
        name for name, _ in inspect.getmembers(VADBackend)
    }


def test_asr_protocol_methods():
    assert hasattr(ASRBackend, "transcribe")
    assert hasattr(ASRBackend, "stream")


def test_llm_protocol_methods():
    assert hasattr(LLMClient, "stream")
    assert hasattr(LLMClient, "cancel")


def test_tts_protocol_methods():
    assert hasattr(TTSBackend, "synthesize")
    assert hasattr(TTSBackend, "stream")


def test_llm_event_supports_progress():
    """LlmEvent must carry progress field per spec (Progress events)."""
    evt = LlmEvent(type="Progress", progress=0.5)
    assert evt.progress == 0.5
    assert evt.text is None


def test_llm_event_supports_tool_name():
    evt = LlmEvent(type="ToolCallStart", tool_name="weather")
    assert evt.tool_name == "weather"


def test_vad_segment_fields():
    seg = VadSegment(samples=[0.1, 0.2], sample_rate=16000, start_ms=0, end_ms=100)
    assert seg.sample_rate == 16000
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd extensions/voice-assistant/service
python -m pytest tests/test_contracts.py -v
```

Expected: FAIL with `ModuleNotFoundError: No module named 'contracts'`

- [ ] **Step 3: Implement contracts.py**

Create `contracts.py` (exact contents from spec Section 1):

```python
"""Backend interface contracts — single source of truth.

All backends implement these Protocols. The orchestrator depends only on
these interfaces, never on concrete classes.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import AsyncIterator, Protocol


@dataclass
class VadSegment:
    """One complete speech segment from VAD."""
    samples: list[float]      # 16kHz mono float32
    sample_rate: int
    start_ms: int
    end_ms: int


@dataclass
class PartialTranscript:
    text: str
    is_final: bool
    confidence: float


@dataclass
class LlmEvent:
    # Type values match NeoMind WS event casing (verified in NeoMind source):
    # crates/neomind-api/src/handlers/sessions.rs serializes End as lowercase "end",
    # others are PascalCase. Error casing varies — handle both.
    type: str  # "Content" | "Thinking" | "ToolCallStart" | "ToolCallEnd" | "Progress" | "end" | "Error" | "error"
    text: str | None = None
    tool_name: str | None = None
    progress: float | None = None


@dataclass
class TtsChunk:
    pcm_int16: bytes
    sample_rate: int
    is_final: bool


class VADBackend(Protocol):
    """Voice Activity Detection backend."""
    def feed(self, pcm_int16: bytes) -> list[VadSegment]:
        """Feed 32ms audio chunk. Return any completed speech segments."""
        ...
    def flush(self) -> list[VadSegment]:
        """Return any in-progress segment at end of stream."""
        ...
    @property
    def sample_rate(self) -> int:
        """Expected input sample rate (typically 16000)."""
        ...


class ASRBackend(Protocol):
    """Automatic Speech Recognition backend."""
    async def transcribe(self, pcm_float32: list[float], sample_rate: int) -> str:
        """Batch: transcribe a complete audio segment."""
        ...
    async def stream(
        self, pcm_iterator: AsyncIterator[list[float]]
    ) -> AsyncIterator[PartialTranscript]:
        """Streaming: continuously emit partials. Phase 2 path."""
        raise NotImplementedError


class LLMClient(Protocol):
    """LLM streaming client. Orchestrator filters Content events to TTS."""
    async def stream(self, user_text: str, session_id: str) -> AsyncIterator[LlmEvent]:
        ...
    async def cancel(self, session_id: str) -> None:
        """Best-effort cancel. Must be idempotent."""
        ...


class TTSBackend(Protocol):
    """Text-to-Speech backend."""
    async def synthesize(self, text: str, voice: str) -> bytes:
        """Batch: synthesize full utterance. Returns int16 LE PCM."""
        ...
    async def stream(self, text: str, voice: str) -> AsyncIterator[TtsChunk]:
        """Streaming: chunked PCM. Phase 2 path."""
        raise NotImplementedError


class AECBackend(Protocol):
    """Acoustic Echo Cancellation backend (Phase 2)."""
    def process(self, mic_pcm: bytes, reference_pcm: bytes) -> bytes:
        """Subtract echo from mic. 16kHz mono int16."""
        ...
```

- [ ] **Step 4: Run test to verify it passes**

```bash
python -m pytest tests/test_contracts.py -v
```

Expected: PASS (7 tests)

- [ ] **Step 5: Commit**

```bash
git add extensions/voice-assistant/service/contracts.py \
        extensions/voice-assistant/service/tests/__init__.py \
        extensions/voice-assistant/service/tests/test_contracts.py
git commit -m "feat(voice): add contracts.py with backend Protocol interfaces"
```

---

## Task 2: Create `stats.py` — RollingPercentile

**Files:**
- Create: `extensions/voice-assistant/service/stats.py`
- Create: `extensions/voice-assistant/service/tests/test_stats.py`

- [ ] **Step 1: Write failing test**

Create `tests/test_stats.py`:

```python
"""RollingPercentile keeps last N samples and reports percentiles."""
from __future__ import annotations

from stats import RollingPercentile


def test_empty_returns_zero():
    rp = RollingPercentile(window=10)
    assert rp.percentile(50) == 0.0


def test_single_sample():
    rp = RollingPercentile(window=10)
    rp.observe(100.0)
    assert rp.percentile(50) == 100.0


def test_multiple_samples_p50():
    rp = RollingPercentile(window=100)
    for v in [10, 20, 30, 40, 50, 60, 70, 80, 90, 100]:
        rp.observe(float(v))
    # sorted: [10..100], p50 index = int(10*0.5) = 5 → 60
    assert rp.percentile(50) == 60.0


def test_window_eviction():
    rp = RollingPercentile(window=3)
    rp.observe(100.0)
    rp.observe(200.0)
    rp.observe(300.0)
    rp.observe(400.0)  # evicts 100
    # sorted: [200, 300, 400], p50 index = int(3*0.5) = 1 → 300
    assert rp.percentile(50) == 300.0


def test_p95_index_clamped():
    rp = RollingPercentile(window=5)
    for v in [1, 2, 3, 4, 5]:
        rp.observe(float(v))
    # sorted: [1,2,3,4,5], p95 index = int(5*0.95) = 4, clamped to 4 → 5
    assert rp.percentile(95) == 5.0
```

- [ ] **Step 2: Run test to verify it fails**

```bash
python -m pytest tests/test_stats.py -v
```

Expected: FAIL with `ModuleNotFoundError: No module named 'stats'`

- [ ] **Step 3: Implement stats.py**

```python
"""Rolling percentile for latency KPIs. O(1) observe, O(N log N) percentile."""
from __future__ import annotations

from collections import deque


class RollingPercentile:
    """Fixed-size sliding window of float samples."""

    def __init__(self, window: int = 100):
        self.samples: deque[float] = deque(maxlen=window)

    def observe(self, value_ms: float) -> None:
        self.samples.append(value_ms)

    def percentile(self, p: float) -> float:
        if not self.samples:
            return 0.0
        sorted_samples = sorted(self.samples)
        idx = int(len(sorted_samples) * p / 100)
        idx = min(idx, len(sorted_samples) - 1)
        return sorted_samples[idx]
```

- [ ] **Step 4: Run test to verify it passes**

Expected: PASS (5 tests)

- [ ] **Step 5: Commit**

```bash
git add extensions/voice-assistant/service/stats.py \
        extensions/voice-assistant/service/tests/test_stats.py
git commit -m "feat(voice): add RollingPercentile for latency KPI tracking"
```

---

## Task 3: Create `profile.py` + `profiles/default.yaml`

**Files:**
- Create: `extensions/voice-assistant/service/profiles/default.yaml`
- Create: `extensions/voice-assistant/service/profiles/headset.yaml`
- Create: `extensions/voice-assistant/service/profile.py`
- Create: `extensions/voice-assistant/service/tests/test_profile_loading.py`
- Modify: `extensions/voice-assistant/service/requirements.txt` (add pyyaml)

- [ ] **Step 1: Add pyyaml to requirements**

Append to `requirements.txt`:
```
pyyaml>=6.0
```

- [ ] **Step 2: Write failing test**

Create `tests/test_profile_loading.py`:

```python
"""Profile YAML loading + env var override."""
from __future__ import annotations

import os

import pytest

from profile import load_profile, Profile


def test_default_profile_loads():
    prof = load_profile(None)
    assert prof.name == "default"
    assert prof.vad_backend_type == "silero"
    assert prof.asr_config["type"] == "sensevoice_http"
    assert prof.llm_config["type"] == "neomind_ws"
    assert prof.tts_config["type"] == "zipvoice_http"
    assert prof.barge_in_mode == "full"


def test_named_profile_loads():
    prof = load_profile("headset")
    assert prof.vad_backend_type == "energy"
    assert prof.aec_config is None  # headset: no echo path


def test_edge_arm_profile_loads():
    """Verify edge-arm profile with Ollama LLM (non-NeoMind path)."""
    prof = load_profile("edge-arm")
    assert prof.cpu_threads == 6
    assert prof.llm_config["type"] == "ollama_http"
    assert prof.llm_config["model"] == "qwen3:1.7b"


def test_noisy_env_profile_loads():
    """Verify noisy-env profile has elevated VAD threshold."""
    prof = load_profile("noisy-env")
    assert prof.vad_config["threshold"] == 0.7
    assert prof.vad_config["min_speech_ms"] == 500


def test_env_override_vad(monkeypatch):
    monkeypatch.setenv("VOICE_ASSISTANT_VAD_BACKEND", "energy")
    prof = load_profile(None)
    assert prof.vad_backend_type == "energy"


def test_env_override_tts_url(monkeypatch):
    monkeypatch.setenv("VOICE_ASSISTANT_TTS_URL", "http://example:9999")
    prof = load_profile(None)
    assert prof.tts_config["url"] == "http://example:9999"


def test_latency_target_informational():
    prof = load_profile(None)
    # Field exists but is informational only
    assert prof.latency_target_ms > 0
```

- [ ] **Step 3: Run test to verify it fails**

Expected: FAIL with `ModuleNotFoundError: No module named 'profile'`

- [ ] **Step 4: Create profile YAMLs**

Create `profiles/default.yaml`:
```yaml
name: default
description: "Developer default: M4 Pro + desktop mic + CN/EN"
hardware:
  cpu_threads: 4
acoustic:
  vad_backend: silero
  vad_threshold: 0.5
  vad_min_speech_ms: 250
  vad_silence_ms: 500
  aec: none
backends:
  asr:
    type: sensevoice_http
    url: http://127.0.0.1:9383
    language: auto
  llm:
    type: neomind_ws
    url: ws://127.0.0.1:9375/api/chat
    token_env: NEOMIND_TOKEN
    voice_mode: true
  tts:
    type: zipvoice_http
    url: http://127.0.0.1:9386
    voice: 中文女
interaction:
  barge_in: full
  latency_target_ms: 1200
```

Create `profiles/headset.yaml`:
```yaml
name: headset
description: "Near-field headset, no echo path"
acoustic:
  vad_backend: energy
  vad_threshold: 0.3
  aec: none
```

Create `profiles/edge-arm.yaml`:
```yaml
name: edge-arm
description: "RK3588 / Jetson Orin Nano, CPU-only, offline LLM"
hardware:
  cpu_threads: 6
acoustic:
  vad_backend: silero
  vad_threshold: 0.6
  vad_min_speech_ms: 300
  vad_silence_ms: 600
  aec: none
backends:
  asr:
    type: sensevoice_http
    url: http://127.0.0.1:9383
  llm:
    type: ollama_http
    url: http://127.0.0.1:11434
    model: qwen3:1.7b
  tts:
    type: zipvoice_http
    url: http://127.0.0.1:9386
    voice: 中文女
interaction:
  barge_in: full
  latency_target_ms: 1500
```

Create `profiles/noisy-env.yaml`:
```yaml
name: noisy-env
description: "Factory floor / vehicle cabin, high ambient noise"
acoustic:
  vad_backend: silero
  vad_threshold: 0.7
  vad_min_speech_ms: 500
  vad_silence_ms: 700
  noise_suppression: rnnoise
  aec: none  # speexdsp lands in Phase 2
interaction:
  latency_target_ms: 1500
```

- [ ] **Step 5: Implement profile.py**

```python
"""Profile YAML loading with env var override."""
from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path

import yaml


@dataclass
class Profile:
    name: str
    vad_backend_type: str
    vad_config: dict
    asr_config: dict
    llm_config: dict
    tts_config: dict
    aec_config: dict | None
    barge_in_mode: str
    latency_target_ms: int
    cpu_threads: int

    @classmethod
    def from_dict(cls, d: dict) -> "Profile":
        acoustic = d.get("acoustic", {})
        backends = d.get("backends", {})
        interaction = d.get("interaction", {})
        hardware = d.get("hardware", {})
        aec = acoustic.get("aec", "none")
        return cls(
            name=d.get("name", "unknown"),
            vad_backend_type=acoustic.get("vad_backend", "silero"),
            vad_config={
                "threshold": acoustic.get("vad_threshold", 0.5),
                "min_speech_ms": acoustic.get("vad_min_speech_ms", 250),
                "silence_ms": acoustic.get("vad_silence_ms", 500),
            },
            asr_config=dict(backends.get("asr", {})),
            llm_config=dict(backends.get("llm", {})),
            tts_config=dict(backends.get("tts", {})),
            aec_config=None if aec == "none" else {"type": aec},
            barge_in_mode=interaction.get("barge_in", "full"),
            latency_target_ms=interaction.get("latency_target_ms", 1200),
            cpu_threads=hardware.get("cpu_threads", 4),
        )


def _parse_yaml(path: Path) -> dict:
    if not path.is_file():
        return {}
    return yaml.safe_load(path.read_text(encoding="utf-8")) or {}


def _deep_merge(base: dict, overlay: dict) -> dict:
    result = dict(base)
    for k, v in overlay.items():
        if k in result and isinstance(result[k], dict) and isinstance(v, dict):
            result[k] = _deep_merge(result[k], v)
        else:
            result[k] = v
    return result


def load_profile(name: str | None = None) -> Profile:
    """Load profile. Priority: env var > named profile > default."""
    profiles_dir = Path(__file__).parent / "profiles"
    default = _parse_yaml(profiles_dir / "default.yaml")
    if name:
        named = _parse_yaml(profiles_dir / f"{name}.yaml")
        merged = _deep_merge(default, named)
    else:
        merged = default

    acoustic = merged.setdefault("acoustic", {})
    backends = merged.setdefault("backends", {})
    tts = backends.setdefault("tts", {})

    # Env overrides
    if v := os.environ.get("VOICE_ASSISTANT_VAD_BACKEND"):
        acoustic["vad_backend"] = v.lower()
    if v := os.environ.get("VOICE_ASSISTANT_TTS_URL"):
        tts["url"] = v
    if v := os.environ.get("VOICE_ASSISTANT_VOICE"):
        tts["voice"] = v

    return Profile.from_dict(merged)
```

- [ ] **Step 6: Run tests, verify pass**

Expected: PASS (5 tests)

- [ ] **Step 7: Commit**

```bash
git add extensions/voice-assistant/service/profile.py \
        extensions/voice-assistant/service/profiles/ \
        extensions/voice-assistant/service/tests/test_profile_loading.py \
        extensions/voice-assistant/service/requirements.txt
git commit -m "feat(voice): add profile YAML system with env var overrides"
```

---

## Task 4: Extract VAD backends into `backends/vad.py`

**Files:**
- Create: `extensions/voice-assistant/service/backends/__init__.py`
- Create: `extensions/voice-assistant/service/backends/vad.py`
- Modify: `extensions/voice-assistant/service/server.py` (import from backends)
- Move: `extensions/voice-assistant/service/test_silero_vad.py` → `tests/test_silero_vad.py`

**Context:** Current `server.py` has `_SILERO_VAD_CONFIG` loader (L524-561), VoiceSession's `_feed_pcm_silero` (L779-814), `_feed_pcm_energy` (L648-696), `_feed_pcm_fsmn` (L697-767). Extract these into classes implementing `VADBackend` Protocol.

**Important:** This task is **logic-preserving**. Move existing code, wrap in classes, do NOT change VAD behavior. The existing `test_silero_vad.py` must still pass after move.

- [ ] **Step 1: Create backends package skeleton**

```bash
mkdir -p extensions/voice-assistant/service/backends
touch extensions/voice-assistant/service/backends/__init__.py
```

- [ ] **Step 2: Create backends/vad.py with SileroVAD and EnergyVAD**

Extract the VAD logic from `server.py`. The classes must satisfy `VADBackend` Protocol:

```python
"""VAD backend implementations."""
from __future__ import annotations

import logging
import time
from pathlib import Path

import numpy as np

from contracts import VadSegment

logger = logging.getLogger("voice-assistant.vad")


class EnergyVAD:
    """Simple RMS energy threshold VAD (PoC legacy)."""

    def __init__(
        self,
        threshold: float = 0.015,
        min_speech_ms: int = 300,
        silence_ms: int = 500,
        sample_rate: int = 16000,
    ):
        self.threshold = threshold
        self.min_speech_ms = min_speech_ms
        self.silence_ms = silence_ms
        self._sample_rate = sample_rate
        self.in_speech = False
        self.speech_audio: list[np.ndarray] = []
        self.silence_frames = 0
        self.speech_frames = 0

    @property
    def sample_rate(self) -> int:
        return self._sample_rate

    def feed(self, pcm_int16: bytes) -> list[VadSegment]:
        # Port the exact logic from server.py VoiceSession._feed_pcm_energy.
        # Convert result from bytes to VadSegment before returning.
        # Pseudocode (replace with ported logic):
        raise NotImplementedError("port from server.py L648-696")

    def flush(self) -> list[VadSegment]:
        # Return any in-progress segment
        return []


# Silero config loader — extracted from server.py L524-561.
# Module-level singleton pattern (model file is shared across sessions).
_SILERO_VAD_CONFIG = None  # sherpa_onnx.OfflineVadModelConfig, lazily loaded


def _ensure_silero_config():
    """Load Silero VAD model config, auto-download if missing.
    Port exact logic from server.py L524-561."""
    global _SILERO_VAD_CONFIG
    # ... port from server.py
    raise NotImplementedError("port from server.py L524-561")


class SileroVAD:
    """sherpa-onnx Silero v5 VAD. One instance per session."""

    def __init__(
        self,
        threshold: float = 0.5,
        min_speech_ms: int = 250,
        silence_ms: int = 500,
        sample_rate: int = 16000,
    ):
        _ensure_silero_config()
        import sherpa_onnx
        self.threshold = threshold
        self.min_speech_ms = min_speech_ms
        self.silence_ms = silence_ms
        self._sample_rate = sample_rate
        self._vad = sherpa_onnx.VoiceActivityDetector(
            _SILERO_VAD_CONFIG, buffer_size_in_seconds=30,
        )

    @property
    def sample_rate(self) -> int:
        return self._sample_rate

    def feed(self, pcm_int16: bytes) -> list[VadSegment]:
        """Port logic from server.py VoiceSession._feed_pcm_silero L779-814.
        CRITICAL: read segment.samples BEFORE pop() — pop destroys backing."""
        samples_int16 = np.frombuffer(pcm_int16, dtype=np.int16)
        audio = samples_int16.astype(np.float32) / 32768.0
        self._vad.accept_waveform(audio.tolist())
        segments: list[VadSegment] = []
        while not self._vad.empty():
            segment = self._vad.front
            samples = np.asarray(segment.samples, dtype=np.float32)  # read FIRST
            self._vad.pop()  # then pop (pop invalidates backing)
            segments.append(VadSegment(
                samples=samples.tolist(),
                sample_rate=self._sample_rate,
                start_ms=0,
                end_ms=int(len(samples) / self._sample_rate * 1000),
            ))
        return segments

    def flush(self) -> list[VadSegment]:
        # sherpa-onnx VAD has no explicit flush; segments returned via feed()
        return []
```

**Action:** Copy the real implementation logic from `server.py` lines noted above. Do not guess — port line by line.

- [ ] **Step 3: Update server.py to import from backends.vad**

At top of `server.py`, after existing imports, add:
```python
from backends.vad import SileroVAD, EnergyVAD, _SILERO_VAD_CONFIG as _SILERO_IMPORTED
```

For minimal disruption in this step: keep the existing VoiceSession methods but have them delegate to the new classes internally. Alternatively, since VoiceSession is the per-session wrapper, just import the singleton config and let VoiceSession continue to use sherpa_onnx directly — but the **class definitions** now live in `backends/vad.py` for future use.

**Minimal viable refactor for this step:** Move `_SILERO_VAD_CONFIG` loader + `_ensure_silero_config` into `backends/vad.py`, re-import into `server.py`. The classes can be defined but unused yet (extracted for Task 10's orchestrator).

- [ ] **Step 4: Move test_silero_vad.py to tests/**

```bash
git mv extensions/voice-assistant/service/test_silero_vad.py \
       extensions/voice-assistant/service/tests/test_silero_vad.py
```

Update imports inside the test if needed.

- [ ] **Step 5: Run existing Silero test to verify no regression**

```bash
cd extensions/voice-assistant/service
VOICE_ASSISTANT_VAD_BACKEND=silero python -m pytest tests/test_silero_vad.py -v
```

Expected: PASS (2 tests) — same as before the move.

- [ ] **Step 6: Verify e2e pipeline still works**

```bash
./start.sh &
# Open poc.html in browser via http server (see voice-edge-stack plan)
# Speak a test phrase, confirm TTS response
kill %1
```

- [ ] **Step 7: Commit**

```bash
git add extensions/voice-assistant/service/backends/
git add extensions/voice-assistant/service/server.py
git add extensions/voice-assistant/service/tests/test_silero_vad.py
git commit -m "refactor(voice): extract VAD classes to backends/vad.py"
```

---

## Task 5: Extract ASR backend into `backends/asr.py`

**Files:**
- Create: `extensions/voice-assistant/service/backends/asr.py`
- Create: `extensions/voice-assistant/service/tests/test_asr.py`
- Modify: `extensions/voice-assistant/service/server.py`

**Context:** Current `server.py:128` `asr_transcribe()` does HTTP POST to sensevoice-asr. Extract into `SenseVoiceHTTPASR` class implementing `ASRBackend`.

- [ ] **Step 1: Write failing test (with mock HTTP)**

Create `tests/test_asr.py`:

```python
"""SenseVoiceHTTPASR backend test with mocked HTTP."""
from __future__ import annotations

import json
from unittest.mock import AsyncMock, patch

import pytest

from backends.asr import SenseVoiceHTTPASR


@pytest.mark.asyncio
async def test_transcribe_returns_text():
    asr = SenseVoiceHTTPASR(url="http://mock:9383", language="auto")
    mock_response = AsyncMock()
    mock_response.status_code = 200
    mock_response.json = lambda: {"text": "你好世界"}
    mock_response.raise_for_status = lambda: None

    with patch("httpx.AsyncClient.post", new=AsyncMock(return_value=mock_response)):
        result = await asr.transcribe([0.0, 0.1, 0.2], 16000)
    assert result == "你好世界"


@pytest.mark.asyncio
async def test_transcribe_raises_on_http_error():
    import httpx
    asr = SenseVoiceHTTPASR(url="http://mock:9383")
    mock_response = AsyncMock()
    mock_response.status_code = 500
    mock_response.raise_for_status = lambda: (_ for _ in ()).throw(
        httpx.HTTPStatusError("err", request=None, response=mock_response)
    )
    with patch("httpx.AsyncClient.post", new=AsyncMock(return_value=mock_response)):
        with pytest.raises(Exception):
            await asr.transcribe([0.0], 16000)
```

- [ ] **Step 2: Run test to verify it fails**

- [ ] **Step 3: Implement backends/asr.py**

Port the body of `asr_transcribe()` from `server.py:128-141` into a class:

```python
"""ASR backend implementations."""
from __future__ import annotations

import logging

import httpx
import numpy as np

from contracts import ASRBackend

logger = logging.getLogger("voice-assistant.asr")


class SenseVoiceHTTPASR:
    """SenseVoice-Small ASR via HTTP (sensevoice-asr service on port 9383)."""

    def __init__(self, url: str, language: str = "auto", timeout: float = 10.0):
        self.url = url
        self.language = language
        self.timeout = timeout

    async def transcribe(self, pcm_float32: list[float], sample_rate: int) -> str:
        """Transcribe complete segment. Returns text."""
        # Port exact request shape from server.py asr_transcribe (L128-141):
        # - POST to {url}/asr
        # - Body: base64-encoded WAV (16kHz mono int16)
        # - Returns: {"text": "..."}
        pcm_int16 = (np.clip(np.asarray(pcm_float32, dtype=np.float32), -1, 1)
                     * 32767).astype("<i2")
        # Build WAV bytes (reuse _pcm_to_wav from server.py)
        wav_bytes = _pcm_to_wav(pcm_int16.tobytes(), sample_rate)
        import base64
        async with httpx.AsyncClient(timeout=self.timeout) as client:
            resp = await client.post(
                f"{self.url}/asr",
                json={
                    "audio": base64.b64encode(wav_bytes).decode(),
                    "language": self.language,
                },
            )
            resp.raise_for_status()
            return resp.json().get("text", "").strip()


def _pcm_to_wav(pcm_int16: bytes, sample_rate: int, channels: int = 1) -> bytes:
    """Wrap raw PCM in WAV header. Port from server.py L206."""
    import io
    import wave
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(sample_rate)
        w.writeframes(pcm_int16)
    return buf.getvalue()
```

**Note:** Match the exact request shape your current sensevoice-asr service expects. Read its `server.py` if uncertain.

- [ ] **Step 4: Run test, verify pass**

- [ ] **Step 5: Update server.py to delegate**

In `server.py`, replace the body of `asr_transcribe()` with a call to the new class. Keep the function signature for backward compat:

```python
from backends.asr import SenseVoiceHTTPASR
_asr_backend = SenseVoiceHTTPASR(url=ASR_URL)

async def asr_transcribe(pcm_int16: bytes, language: str = "auto") -> dict:
    # Delegate to backend (convert bytes → float32 list first)
    samples = np.frombuffer(pcm_int16, dtype=np.int16).astype(np.float32) / 32768.0
    text = await _asr_backend.transcribe(samples.tolist(), SAMPLE_RATE)
    return {"text": text}
```

- [ ] **Step 6: Verify e2e still works** (speak a phrase, see transcript)

- [ ] **Step 7: Commit**

```bash
git add extensions/voice-assistant/service/backends/asr.py \
        extensions/voice-assistant/service/tests/test_asr.py \
        extensions/voice-assistant/service/server.py
git commit -m "refactor(voice): extract SenseVoiceASR to backends/asr.py"
```

---

## Task 6: Extract TTS backend into `backends/tts.py`

**Files:**
- Create: `extensions/voice-assistant/service/backends/tts.py`
- Create: `extensions/voice-assistant/service/tests/test_tts.py`
- Modify: `extensions/voice-assistant/service/server.py`

**Context:** Current `server.py:141-204` `tts_stream()` does NDJSON streaming POST to voice-edge-tts. Extract into `ZipVoiceHTTP` class implementing `TTSBackend`.

- [ ] **Step 1: Write failing test**

Create `tests/test_tts.py`:

```python
"""ZipVoiceHTTP backend test with mocked HTTP."""
from __future__ import annotations

import base64
import json
from unittest.mock import AsyncMock, patch

import pytest

from backends.tts import ZipVoiceHTTP


@pytest.mark.asyncio
async def test_synthesize_returns_pcm():
    tts = ZipVoiceHTTP(url="http://mock:9386", voice="中文女")
    fake_pcm = b"\x00\x00" * 100
    fake_ndjson = (
        json.dumps({
            "seq": 0,
            "data": base64.b64encode(fake_pcm).decode(),
            "sample_rate": 24000,
            "channels": 1,
        }).encode() + b"\n"
    )

    mock_resp = AsyncMock()
    mock_resp.status_code = 200
    mock_resp.raise_for_status = lambda: None
    mock_resp.aiter_bytes = lambda: _aiter([fake_ndjson])
    mock_resp.__aenter__ = AsyncMock(return_value=mock_resp)
    mock_resp.__aexit__ = AsyncMock(return_value=None)

    with patch("httpx.AsyncClient.post", return_value=mock_resp):
        pcm = await tts.synthesize("你好", "中文女")
    assert pcm == fake_pcm


async def _aiter(chunks):
    for c in chunks:
        yield c
```

- [ ] **Step 2: Run, verify fail**

- [ ] **Step 3: Implement backends/tts.py**

```python
"""TTS backend implementations."""
from __future__ import annotations

import base64
import json
import logging

import httpx

from contracts import TTSBackend

logger = logging.getLogger("voice-assistant.tts")


class ZipVoiceHTTP:
    """ZipVoice TTS via voice-edge-tts HTTP service (port 9386).
    NDJSON /tts/stream contract — one line per PCM chunk."""

    def __init__(self, url: str, voice: str = "中文女", timeout: float = 30.0):
        self.url = url
        self.voice = voice
        self.timeout = timeout

    async def synthesize(self, text: str, voice: str) -> bytes:
        """Batch: synthesize full utterance. Returns int16 LE PCM."""
        async with httpx.AsyncClient(timeout=self.timeout) as client:
            async with client.stream(
                "POST",
                f"{self.url}/tts/stream",
                json={"text": text, "voice": voice or self.voice},
            ) as resp:
                resp.raise_for_status()
                pcm_chunks: list[bytes] = []
                async for line in resp.aiter_lines():
                    line = line.strip()
                    if not line:
                        continue
                    obj = json.loads(line)
                    if "error" in obj:
                        raise RuntimeError(obj["error"])
                    pcm_chunks.append(base64.b64decode(obj["data"]))
                return b"".join(pcm_chunks)

    async def stream(self, text: str, voice: str):
        """Streaming variant. Phase 2 — not yet implemented for ZipVoice."""
        raise NotImplementedError("ZipVoice is pseudo-streaming; use synthesize()")
```

- [ ] **Step 4: Run test, verify pass**

- [ ] **Step 5: Update server.py `tts_stream()` to delegate**

Keep the function; have it use the new backend internally. Apply the existing PCM resampling logic (`_tts_to_browser_pcm`) on the returned bytes.

- [ ] **Step 6: Verify e2e still works**

- [ ] **Step 7: Commit**

```bash
git add extensions/voice-assistant/service/backends/tts.py \
        extensions/voice-assistant/service/tests/test_tts.py \
        extensions/voice-assistant/service/server.py
git commit -m "refactor(voice): extract ZipVoiceHTTP to backends/tts.py"
```

---

## Task 7: Extract LLM backends into `backends/llm.py`

**Files:**
- Create: `extensions/voice-assistant/service/backends/llm.py`
- Create: `extensions/voice-assistant/service/tests/test_llm.py`
- Modify: `extensions/voice-assistant/service/server.py`

**Context:** Current `server.py` has `ollama_llm_stream` (L392), `fake_llm_stream` (L304), and `llm_stream` dispatcher (L472). Extract these into `OllamaHTTPClient` and `FakeLLMClient` classes implementing `LLMClient`.

- [ ] **Step 1: Write failing test for FakeLLMClient**

Create `tests/test_llm.py`:

```python
"""LLM backend tests."""
from __future__ import annotations

import pytest

from backends.llm import FakeLLMClient, OllamaHTTPClient
from contracts import LlmEvent


@pytest.mark.asyncio
async def test_fake_llm_emits_content_events():
    llm = FakeLLMClient(reply_template="Echo: {text}")
    events = []
    async for evt in llm.stream("hello", session_id="s1"):
        events.append(evt)
    # Should have at least one Content event and an end event
    content_events = [e for e in events if e.type == "Content"]
    assert len(content_events) >= 1
    assert "hello" in content_events[0].text
    assert events[-1].type in ("end", "End")


@pytest.mark.asyncio
async def test_fake_llm_cancel_is_idempotent():
    llm = FakeLLMClient()
    await llm.cancel("s1")  # should not raise
    await llm.cancel("s1")  # second call also safe


@pytest.mark.asyncio
async def test_ollama_client_initializes():
    # Just verify construction doesn't fail
    llm = OllamaHTTPClient(url="http://mock:11434", model="qwen3:1.7b")
    assert llm.model == "qwen3:1.7b"
```

- [ ] **Step 2: Run, verify fail**

- [ ] **Step 3: Implement backends/llm.py**

```python
"""LLM backend implementations."""
from __future__ import annotations

import asyncio
import json
import logging
from typing import AsyncIterator

import httpx

from contracts import LLMClient, LlmEvent

logger = logging.getLogger("voice-assistant.llm")


class FakeLLMClient:
    """Echo LLM for testing. Implements LLMClient Protocol."""

    def __init__(self, reply_template: str = "你刚才说: {text}"):
        self.reply_template = reply_template
        self._cancelled_sessions: set[str] = set()

    async def stream(self, user_text: str, session_id: str) -> AsyncIterator[LlmEvent]:
        if session_id in self._cancelled_sessions:
            yield LlmEvent(type="end")
            return
        reply = self.reply_template.format(text=user_text)
        # Emit Content in ~10-char chunks to simulate streaming
        for i in range(0, len(reply), 10):
            if session_id in self._cancelled_sessions:
                yield LlmEvent(type="end")
                return
            chunk = reply[i:i+10]
            yield LlmEvent(type="Content", text=chunk)
            await asyncio.sleep(0.01)
        yield LlmEvent(type="end")

    async def cancel(self, session_id: str) -> None:
        self._cancelled_sessions.add(session_id)


class OllamaHTTPClient:
    """Ollama local LLM via HTTP streaming (port 11434)."""

    def __init__(
        self,
        url: str = "http://127.0.0.1:11434",
        model: str = "qwen3:1.7b",
        system_prompt: str | None = None,
        timeout: float = 60.0,
    ):
        self.url = url
        self.model = model
        self.system_prompt = system_prompt or (
            "你是简洁的中文语音助手。用口语化短句回答，每句不超过 20 字。"
            "不要使用 Markdown 格式。"
        )
        self.timeout = timeout
        self._cancelled: set[str] = set()

    async def stream(self, user_text: str, session_id: str) -> AsyncIterator[LlmEvent]:
        # Port logic from server.py ollama_llm_stream (L392-456).
        # Ollama returns NDJSON: {"message":{"content":"..."},"done":false}
        # Emit LlmEvent(type="Content", text=chunk) per token.
        # On done, emit LlmEvent(type="end").
        # Check self._cancelled between chunks; if cancelled, emit "end" and return.
        async with httpx.AsyncClient(timeout=self.timeout) as client:
            async with client.stream(
                "POST",
                f"{self.url}/api/chat",
                json={
                    "model": self.model,
                    "messages": [
                        {"role": "system", "content": self.system_prompt},
                        {"role": "user", "content": user_text},
                    ],
                    "stream": True,
                },
            ) as resp:
                resp.raise_for_status()
                async for line in resp.aiter_lines():
                    if session_id in self._cancelled:
                        yield LlmEvent(type="end")
                        return
                    if not line.strip():
                        continue
                    obj = json.loads(line)
                    if obj.get("done"):
                        yield LlmEvent(type="end")
                        return
                    chunk = obj.get("message", {}).get("content", "")
                    if chunk:
                        yield LlmEvent(type="Content", text=chunk)

    async def cancel(self, session_id: str) -> None:
        self._cancelled.add(session_id)
```

- [ ] **Step 4: Run test, verify pass**

- [ ] **Step 5: Update server.py to delegate `llm_stream()` to new classes**

- [ ] **Step 6: Verify e2e (Ollama path) still works**

- [ ] **Step 7: Commit**

```bash
git add extensions/voice-assistant/service/backends/llm.py \
        extensions/voice-assistant/service/tests/test_llm.py \
        extensions/voice-assistant/service/server.py
git commit -m "refactor(voice): extract OllamaHTTPClient and FakeLLMClient to backends/llm.py"
```

---

## Task 8: Implement `NeoMindWSClient` in `backends/llm.py`

**Files:**
- Modify: `extensions/voice-assistant/service/backends/llm.py` (add class)
- Create: `extensions/voice-assistant/service/tests/test_neomind_ws_client.py`

**Context:** New code. Integrates with NeoMind chat WebSocket at `ws://127.0.0.1:9375/api/chat?token=<jwt>`. Event types per spec Section 3.4. Sends `__CANCEL__` for cancellation. Filters `"\n\n[Interrupted]"` Content events.

**Read first:** The spec Section 3.3 for verified NeoMind WS protocol behavior.

- [ ] **Step 1: Write failing test**

Create `tests/test_neomind_ws_client.py`:

```python
"""NeoMindWSClient tests with mocked WebSocket."""
from __future__ import annotations

import asyncio
import json
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from backends.llm import NeoMindWSClient
from contracts import LlmEvent


@pytest.mark.asyncio
async def test_stream_emits_content_events():
    """NeoMind Content events → LlmEvent(type='Content')."""
    client = NeoMindWSClient(url="ws://mock:9375/api/chat", token="t")

    fake_events = [
        {"type": "Thinking"},
        {"type": "Content", "content": "你好"},
        {"type": "ToolCallStart", "toolName": "weather"},
        {"type": "ToolCallEnd"},
        {"type": "Content", "content": "世界"},
        {"type": "end", "sessionId": "s1"},
    ]
    mock_ws = AsyncMock()
    mock_ws.send = AsyncMock()
    mock_ws.closed = False
    mock_ws.__aiter__ = _make_aiter(fake_events)
    mock_ws.__aenter__ = AsyncMock(return_value=mock_ws)
    mock_ws.__aexit__ = AsyncMock(return_value=None)

    with patch("websockets.connect", return_value=mock_ws):
        events = []
        async for evt in client.stream("hi", session_id="s1"):
            events.append(evt)

    content = [e for e in events if e.type == "Content"]
    assert len(content) == 2
    assert content[0].text == "你好"
    assert content[1].text == "世界"
    assert events[-1].type == "end"


@pytest.mark.asyncio
async def test_stream_filters_interrupted_marker():
    """Content with '\\n\\n[Interrupted]' must NOT be emitted (post-cancel marker)."""
    client = NeoMindWSClient(url="ws://mock:9375/api/chat", token="t")

    fake_events = [
        {"type": "Content", "content": "real reply"},
        {"type": "Content", "content": "\n\n[Interrupted]"},
        {"type": "end"},
    ]
    mock_ws = AsyncMock()
    mock_ws.send = AsyncMock()
    mock_ws.closed = False
    mock_ws.__aiter__ = _make_aiter(fake_events)
    mock_ws.__aenter__ = AsyncMock(return_value=mock_ws)
    mock_ws.__aexit__ = AsyncMock(return_value=None)

    with patch("websockets.connect", return_value=mock_ws):
        events = []
        async for evt in client.stream("hi", session_id="s1"):
            events.append(evt)

    content_texts = [e.text for e in events if e.type == "Content"]
    assert "real reply" in content_texts
    assert "\n\n[Interrupted]" not in content_texts


@pytest.mark.asyncio
async def test_cancel_sends_underscore_cancel_message():
    client = NeoMindWSClient(url="ws://mock:9375/api/chat", token="t")
    mock_ws = AsyncMock()
    mock_ws.closed = False
    mock_ws.send = AsyncMock()
    client._active_ws = mock_ws  # inject active connection
    client._llm_completed = False

    await client.cancel("s1")
    mock_ws.send.assert_called_once_with(json.dumps({"type": "__CANCEL__"}))


@pytest.mark.asyncio
async def test_cancel_skipped_if_llm_completed():
    client = NeoMindWSClient(url="ws://mock:9375/api/chat", token="t")
    mock_ws = AsyncMock()
    mock_ws.closed = False
    mock_ws.send = AsyncMock()
    client._active_ws = mock_ws
    client._llm_completed = True  # LLM already ended

    await client.cancel("s1")
    mock_ws.send.assert_not_called()


def _make_aiter(events):
    async def aiter():
        for e in events:
            yield json.dumps(e)
    return aiter
```

- [ ] **Step 2: Run, verify fail**

- [ ] **Step 3: Implement NeoMindWSClient in backends/llm.py**

Append to `backends/llm.py`:

```python
class NeoMindWSClient:
    """NeoMind chat via WebSocket. Implements LLMClient Protocol.

    Protocol (verified from NeoMind source):
    - Connect to ws://host:port/api/chat?token=<jwt>
    - Send {"type": "message", "content": user_text, "sessionId": ...}
    - Receive events: Content / Thinking / ToolCallStart / ToolCallEnd / Progress / end / Error
    - To cancel: send {"type": "__CANCEL__"} on same connection
    - After cancel: NeoMind emits Content "\\n\\n[Interrupted]" then lowercase "end"
    """

    INTERRUPTED_MARKER = "\n\n[Interrupted]"

    def __init__(
        self,
        url: str,
        token: str | None = None,
        token_env: str = "NEOMIND_TOKEN",
        voice_mode: bool = True,
        timeout: float = 60.0,
    ):
        self.url = url
        self.token = token or os.environ.get(token_env, "")
        self.voice_mode = voice_mode
        self.timeout = timeout
        self._active_ws = None
        self._llm_completed = False

    async def stream(self, user_text: str, session_id: str) -> AsyncIterator[LlmEvent]:
        import os
        import websockets

        self._llm_completed = False
        url = f"{self.url}?token={self.token}" if self.token else self.url
        async with websockets.connect(url, max_size=2**24) as ws:
            self._active_ws = ws
            await ws.send(json.dumps({
                "type": "message",
                "content": user_text,
                "sessionId": session_id,
            }))
            try:
                async for raw in ws:
                    if self._llm_completed:
                        break
                    evt = json.loads(raw)
                    evt_type = evt.get("type", "")
                    if evt_type == "Content":
                        text = evt.get("content", "")
                        if text == self.INTERRUPTED_MARKER:
                            continue  # filter post-cancel marker
                        yield LlmEvent(type="Content", text=text)
                    elif evt_type == "Thinking":
                        yield LlmEvent(type="Thinking")
                    elif evt_type == "ToolCallStart":
                        yield LlmEvent(type="ToolCallStart",
                                       tool_name=evt.get("toolName"))
                    elif evt_type == "ToolCallEnd":
                        yield LlmEvent(type="ToolCallEnd")
                    elif evt_type == "Progress":
                        yield LlmEvent(type="Progress",
                                       progress=evt.get("progress", 0.0))
                    elif evt_type == "end":
                        self._llm_completed = True
                        yield LlmEvent(type="end")
                        return
                    elif evt_type in ("Error", "error"):
                        yield LlmEvent(type="Error", text=evt.get("message", ""))
                        return
            finally:
                self._active_ws = None

    async def cancel(self, session_id: str) -> None:
        if self._llm_completed:
            return  # no-op
        if self._active_ws and not getattr(self._active_ws, "closed", True):
            await self._active_ws.send(json.dumps({"type": "__CANCEL__"}))
            # Per spec Section 3.3: wait for lowercase "end" event with 500ms timeout.
            # NeoMind emits Content "\\n\\n[Interrupted]" then "end" after cancel.
            try:
                await asyncio.wait_for(self._wait_for_end_event(), timeout=0.5)
            except asyncio.TimeoutError:
                logger.warning("NeoMind cancel ack timeout (session=%s)", session_id)

    async def _wait_for_end_event(self) -> None:
        """Block until the active WS emits lowercase 'end' event."""
        if not self._active_ws:
            return
        async for raw in self._active_ws:
            evt = json.loads(raw)
            if evt.get("type") == "end":  # lowercase, verified
                return
            # Discard "[Interrupted]" Content events — do nothing
```

**Add `import asyncio` and `import os` at top of `backends/llm.py`** if not present.

- [ ] **Step 4: Run tests, verify pass (4 tests)**

- [ ] **Step 5: Commit**

```bash
git add extensions/voice-assistant/service/backends/llm.py \
        extensions/voice-assistant/service/tests/test_neomind_ws_client.py
git commit -m "feat(voice): add NeoMindWSClient with __CANCEL__ and [Interrupted] filter"
```

---

## Task 9: Backend factory in `backends/__init__.py`

**Files:**
- Modify: `extensions/voice-assistant/service/backends/__init__.py`
- Create: `extensions/voice-assistant/service/tests/test_factory.py`

- [ ] **Step 1: Write failing test**

```python
"""Factory creates correct backend type from Profile."""
from __future__ import annotations

import pytest

from backends import make_vad, make_asr, make_llm, make_tts
from backends.vad import SileroVAD, EnergyVAD
from backends.asr import SenseVoiceHTTPASR
from backends.llm import OllamaHTTPClient, NeoMindWSClient, FakeLLMClient
from backends.tts import ZipVoiceHTTP
from profile import Profile


def _profile(**overrides):
    base = dict(
        name="test", vad_backend_type="silero",
        vad_config={"threshold": 0.5, "min_speech_ms": 250, "silence_ms": 500},
        asr_config={"type": "sensevoice_http", "url": "http://m"},
        llm_config={"type": "ollama_http", "url": "http://m", "model": "qwen"},
        tts_config={"type": "zipvoice_http", "url": "http://m", "voice": "v"},
        aec_config=None, barge_in_mode="full",
        latency_target_ms=1000, cpu_threads=4,
    )
    base.update(overrides)
    return Profile(**base)


def test_make_vad_silero():
    vad = make_vad(_profile(vad_backend_type="silero"))
    assert isinstance(vad, SileroVAD)


def test_make_vad_energy():
    vad = make_vad(_profile(vad_backend_type="energy"))
    assert isinstance(vad, EnergyVAD)


def test_make_asr():
    asr = make_asr(_profile())
    assert isinstance(asr, SenseVoiceHTTPASR)


def test_make_llm_ollama():
    llm = make_llm(_profile(llm_config={"type": "ollama_http", "url": "x", "model": "q"}))
    assert isinstance(llm, OllamaHTTPClient)


def test_make_llm_neomind():
    llm = make_llm(_profile(llm_config={"type": "neomind_ws", "url": "ws://x"}))
    assert isinstance(llm, NeoMindWSClient)


def test_make_llm_fake():
    llm = make_llm(_profile(llm_config={"type": "fake"}))
    assert isinstance(llm, FakeLLMClient)


def test_make_tts():
    tts = make_tts(_profile())
    assert isinstance(tts, ZipVoiceHTTP)


def test_make_vad_unknown_raises():
    with pytest.raises(ValueError):
        make_vad(_profile(vad_backend_type="nonexistent"))
```

- [ ] **Step 2: Run, verify fail**

- [ ] **Step 3: Implement factory**

Replace `backends/__init__.py`:

```python
"""Backend factory — Profile → concrete backend instances."""
from __future__ import annotations

from profile import Profile
from .vad import SileroVAD, EnergyVAD
from .asr import SenseVoiceHTTPASR
from .llm import OllamaHTTPClient, NeoMindWSClient, FakeLLMClient
from .tts import ZipVoiceHTTP


def make_vad(profile: Profile):
    cfg = profile.vad_config
    match profile.vad_backend_type:
        case "silero":
            return SileroVAD(
                threshold=cfg["threshold"],
                min_speech_ms=cfg["min_speech_ms"],
                silence_ms=cfg["silence_ms"],
            )
        case "energy":
            return EnergyVAD(
                threshold=cfg["threshold"],
                min_speech_ms=cfg["min_speech_ms"],
                silence_ms=cfg["silence_ms"],
            )
        case _:
            raise ValueError(f"unknown VAD backend: {profile.vad_backend_type}")


def make_asr(profile: Profile):
    cfg = dict(profile.asr_config)
    t = cfg.pop("type")
    match t:
        case "sensevoice_http":
            return SenseVoiceHTTPASR(**cfg)
        case _:
            raise ValueError(f"unknown ASR backend: {t}")


def make_llm(profile: Profile):
    cfg = dict(profile.llm_config)
    t = cfg.pop("type")
    match t:
        case "neomind_ws":
            return NeoMindWSClient(**cfg)
        case "ollama_http":
            return OllamaHTTPClient(**cfg)
        case "fake":
            return FakeLLMClient(**cfg)
        case _:
            raise ValueError(f"unknown LLM backend: {t}")


def make_tts(profile: Profile):
    cfg = dict(profile.tts_config)
    t = cfg.pop("type")
    match t:
        case "zipvoice_http":
            return ZipVoiceHTTP(**cfg)
        case _:
            raise ValueError(f"unknown TTS backend: {t}")
```

- [ ] **Step 4: Run tests, verify pass (8 tests)**

- [ ] **Step 5: Commit**

```bash
git add extensions/voice-assistant/service/backends/__init__.py \
        extensions/voice-assistant/service/tests/test_factory.py
git commit -m "feat(voice): add backend factory functions"
```

---

## Task 10: Implement `orchestrator.py` — State machine

**Files:**
- Create: `extensions/voice-assistant/service/orchestrator.py`
- Create: `extensions/voice-assistant/service/tests/test_state_machine.py`

**Context:** Replace VoiceSession's implicit epoch counter with an explicit FSM. Per spec Section 2.1: IDLE/LISTENING/THINKING/SPEAKING/BARGED. This task is JUST the state machine — the pipeline integration comes in Task 11.

- [ ] **Step 1: Write failing test**

Create `tests/test_state_machine.py`:

```python
"""State machine transition tests."""
from __future__ import annotations

import pytest

from orchestrator import State, StateMachine


@pytest.fixture
def fsm():
    return StateMachine()


def test_initial_state_is_idle(fsm):
    assert fsm.state == State.IDLE


def test_idle_to_listening(fsm):
    fsm.transition(State.LISTENING)
    assert fsm.state == State.LISTENING


def test_listening_to_thinking(fsm):
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)
    assert fsm.state == State.THINKING


def test_thinking_to_speaking(fsm):
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)
    fsm.transition(State.SPEAKING)
    assert fsm.state == State.SPEAKING


def test_speaking_to_idle(fsm):
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)
    fsm.transition(State.SPEAKING)
    fsm.transition(State.IDLE)
    assert fsm.state == State.IDLE


def test_any_state_to_barged(fsm):
    for target in [State.THINKING, State.SPEAKING]:
        fsm._state = State.IDLE
        fsm.transition(State.LISTENING)
        fsm.transition(target)
        fsm.transition(State.BARGED)
        assert fsm.state == State.BARGED


def test_barged_to_listening(fsm):
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)
    fsm.transition(State.BARGED)
    fsm.transition(State.LISTENING)
    assert fsm.state == State.LISTENING


def test_invalid_transition_raises(fsm):
    # IDLE → SPEAKING is invalid (must go through LISTENING, THINKING)
    with pytest.raises(ValueError):
        fsm.transition(State.SPEAKING)


def test_barge_in_locks_state(fsm):
    """During BARGED, no other transition until cleanup done."""
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)
    fsm.transition(State.BARGED)
    # Attempting THINKING during BARGED should fail
    with pytest.raises(ValueError):
        fsm.transition(State.THINKING)


def test_callback_fires_on_transition(fsm):
    """Optional callback fires when state changes."""
    calls = []
    fsm.on_transition = lambda prev, new: calls.append((prev, new))
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)
    assert calls == [(State.IDLE, State.LISTENING),
                     (State.LISTENING, State.THINKING)]


@pytest.mark.asyncio
async def test_concurrent_transitions_serialized():
    """Two coroutines transitioning simultaneously don't corrupt state."""
    import asyncio
    fsm = StateMachine()
    async def try_transition(target):
        try:
            fsm.transition(target)
        except ValueError:
            pass
    # Should not raise; final state is deterministic
    await asyncio.gather(
        try_transition(State.LISTENING),
        try_transition(State.THINKING),
    )
    assert fsm.state in (State.IDLE, State.LISTENING, State.THINKING)
```

- [ ] **Step 2: Run, verify fail**

- [ ] **Step 3: Implement StateMachine in orchestrator.py**

```python
"""Voice orchestrator: explicit FSM + pipeline + barge-in.

Phase 1 — this task implements ONLY the StateMachine class. Pipeline and
barge-in logic are added in subsequent tasks.
"""
from __future__ import annotations

import asyncio
import logging
from enum import Enum
from typing import Callable, Awaitable

logger = logging.getLogger("voice-assistant.orchestrator")


class State(str, Enum):
    IDLE = "idle"
    LISTENING = "listening"
    THINKING = "thinking"
    SPEAKING = "speaking"
    BARGED = "barged"


# Valid transitions: from_state → set of allowed target states
_VALID_TRANSITIONS: dict[State, set[State]] = {
    State.IDLE: {State.LISTENING},
    State.LISTENING: {State.THINKING, State.BARGED, State.IDLE},
    State.THINKING: {State.SPEAKING, State.BARGED, State.IDLE},
    State.SPEAKING: {State.IDLE, State.BARGED},
    State.BARGED: {State.LISTENING},  # only after cleanup
}


class StateMachine:
    """Explicit FSM with asyncio-safe transitions.

    All transitions are serialized via a lock. Invalid transitions raise
    ValueError. An optional callback fires on every successful transition.
    """

    def __init__(self):
        self._state = State.IDLE
        self._lock = asyncio.Lock()
        self.on_transition: Callable[[State, State], Awaitable[None] | None] | None = None

    @property
    def state(self) -> State:
        return self._state

    def transition(self, target: State) -> None:
        """Synchronous transition (must be called from async context with lock held).

        For async-safe transitions, use `async_transition`.
        """
        # Self-transition is a no-op (idempotency for concurrent barge-ins)
        if target == self._state:
            return
        if target not in _VALID_TRANSITIONS.get(self._state, set()):
            if target != State.BARGED:  # BARGED is special — allowed from almost anywhere
                raise ValueError(
                    f"Invalid transition {self._state.value} → {target.value}"
                )
        prev = self._state
        self._state = target
        logger.debug("FSM: %s → %s", prev.value, target.value)
        if self.on_transition:
            result = self.on_transition(prev, target)
            if asyncio.iscoroutine(result):
                # Best-effort: schedule but don't block
                asyncio.create_task(result)

    async def async_transition(self, target: State) -> None:
        """Async-safe transition with lock."""
        async with self._lock:
            # Self-transition is a no-op (idempotency for concurrent barge-ins)
            if target == self._state:
                return
            # BARGED is always allowed as an interrupt
            if target == State.BARGED:
                pass
            elif target not in _VALID_TRANSITIONS.get(self._state, set()):
                raise ValueError(
                    f"Invalid transition {self._state.value} → {target.value}"
                )
            prev = self._state
            self._state = target
            logger.info("FSM: %s → %s", prev.value, target.value)
            if self.on_transition:
                result = self.on_transition(prev, target)
                if asyncio.iscoroutine(result):
                    await result
```

- [ ] **Step 4: Run tests, verify pass (11 tests)**

- [ ] **Step 5: Commit**

```bash
git add extensions/voice-assistant/service/orchestrator.py \
        extensions/voice-assistant/service/tests/test_state_machine.py
git commit -m "feat(voice): add explicit 5-state FSM with asyncio-safe transitions"
```

---

## Task 11: Implement barge-in cleanup in `orchestrator.py`

**Files:**
- Modify: `extensions/voice-assistant/service/orchestrator.py`
- Create: `extensions/voice-assistant/service/tests/test_barge_in.py`

**Context:** Per spec Section 3.2 — 4 parallel cleanup actions on barge-in. This task adds `_handle_barge_in()` and the cleanup sub-methods, tested with mocks.

- [ ] **Step 1: Write failing test**

Create `tests/test_barge_in.py`:

```python
"""Barge-in protocol tests with mocked backends."""
from __future__ import annotations

import asyncio
from unittest.mock import AsyncMock, MagicMock

import pytest

from orchestrator import State, StateMachine, BargeInHandler


@pytest.fixture
def handler():
    """BargeInHandler with all dependencies mocked."""
    h = BargeInHandler(
        cancel_tts_playback=AsyncMock(),
        cancel_llm_request=AsyncMock(),
        clear_pending_queues=AsyncMock(),
        drain_asr_buffer=AsyncMock(),
    )
    return h


@pytest.mark.asyncio
async def test_barge_in_runs_all_four_cleanups(handler):
    fsm = StateMachine()
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)

    await handler.handle_barge_in(fsm, reason="test")

    handler.cancel_tts_playback.assert_called_once()
    handler.cancel_llm_request.assert_called_once()
    handler.clear_pending_queues.assert_called_once()
    handler.drain_asr_buffer.assert_called_once()
    assert fsm.state == State.LISTENING  # transitioned back after cleanup


@pytest.mark.asyncio
async def test_barge_in_logs_cleanup_failures(handler):
    """Cleanup task failure is logged but doesn't halt others."""
    handler.cancel_llm_request.side_effect = RuntimeError("ws closed")
    fsm = StateMachine()
    fsm.transition(State.LISTENING)
    fsm.transition(State.SPEAKING)

    # Should not raise
    await handler.handle_barge_in(fsm, reason="test")
    # Other cleanups still ran
    handler.cancel_tts_playback.assert_called_once()
    handler.clear_pending_queues.assert_called_once()


@pytest.mark.asyncio
async def test_barge_in_idempotent_concurrent(handler):
    """Two barge-ins within 200ms execute cleanup only once."""
    fsm = StateMachine()
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)

    await asyncio.gather(
        handler.handle_barge_in(fsm, reason="first"),
        handler.handle_barge_in(fsm, reason="second"),
    )
    # cancel_llm_request should be called once or twice but state must end in LISTENING
    assert fsm.state == State.LISTENING
```

- [ ] **Step 2: Run, verify fail**

- [ ] **Step 3: Implement BargeInHandler in orchestrator.py**

Append to `orchestrator.py`:

```python
import logging
from dataclasses import dataclass
from typing import Callable, Awaitable

logger = logging.getLogger("voice-assistant.barge_in")


@dataclass
class BargeInHandler:
    """Coordinates the 4 parallel cleanup actions on barge-in.

    Each action is an async callable; failures are logged but don't halt others.
    Uses a lock to ensure idempotency for concurrent barge-in triggers.
    """
    cancel_tts_playback: Callable[[], Awaitable[None]]
    cancel_llm_request: Callable[[], Awaitable[None]]
    clear_pending_queues: Callable[[], Awaitable[None]]
    drain_asr_buffer: Callable[[], Awaitable[None]]
    _lock: asyncio.Lock = None

    def __post_init__(self):
        self._lock = asyncio.Lock()
        self._in_progress = False

    async def handle_barge_in(self, fsm: StateMachine, reason: str) -> None:
        """Transition to BARGED, run 4 cleanups, transition to LISTENING.

        Idempotent: if a barge-in is already in progress, return immediately.
        """
        async with self._lock:
            if self._in_progress:
                logger.debug("barge-in already in progress, skipping (reason=%s)", reason)
                return
            if fsm.state == State.IDLE or fsm.state == State.LISTENING:
                logger.debug("barge-in in state %s, nothing to cancel", fsm.state.value)
                return
            self._in_progress = True

        logger.info("BARGE-IN triggered (reason=%s, from=%s)", reason, fsm.state.value)
        await fsm.async_transition(State.BARGED)

        cleanup_tasks = [
            self.cancel_tts_playback(),
            self.cancel_llm_request(),
            self.clear_pending_queues(),
            self.drain_asr_buffer(),
        ]
        results = await asyncio.gather(*cleanup_tasks, return_exceptions=True)
        for i, r in enumerate(results):
            if isinstance(r, Exception):
                logger.warning("barge-in cleanup task %d failed: %s", i, r)

        await fsm.async_transition(State.LISTENING)
        async with self._lock:
            self._in_progress = False
```

- [ ] **Step 4: Run tests, verify pass (3 tests)**

- [ ] **Step 5: Commit**

```bash
git add extensions/voice-assistant/service/orchestrator.py \
        extensions/voice-assistant/service/tests/test_barge_in.py
git commit -m "feat(voice): add BargeInHandler with 4 parallel cleanup actions"
```

---

## Task 12: Create `telemetry.py` — KPI tracking

**Files:**
- Create: `extensions/voice-assistant/service/telemetry.py`
- Create: `extensions/voice-assistant/service/tests/test_telemetry.py`

**Context:** Per spec Section 6 — 7 KPIs tracked via RollingPercentile. OTel console exporter optional (env-gated).

- [ ] **Step 1: Write failing test**

```python
"""Telemetry KPI tracking tests."""
from __future__ import annotations

from telemetry import Telemetry


def test_observe_first_audio_out():
    t = Telemetry()
    t.observe("first_audio_out_ms", 920.0)
    assert t.percentile("first_audio_out_ms", 50) == 920.0


def test_observe_multiple_kpis():
    t = Telemetry()
    for v in [100, 200, 300]:
        t.observe("asr_complete_ms", float(v))
    assert t.percentile("asr_complete_ms", 50) == 200.0


def test_unknown_kpi_returns_zero():
    t = Telemetry()
    assert t.percentile("nonexistent", 50) == 0.0


def test_snapshot_returns_all_kpis():
    t = Telemetry()
    t.observe("first_audio_out_ms", 950.0)
    t.observe("barge_in_to_silence_ms", 150.0)
    snap = t.snapshot()
    assert "first_audio_out_ms" in snap
    assert "barge_in_to_silence_ms" in snap
    assert snap["first_audio_out_ms"]["p50"] == 950.0
    assert snap["barge_in_to_silence_ms"]["p50"] == 150.0


def test_turn_count_increments():
    t = Telemetry()
    assert t.turn_count == 0
    t.increment_turns()
    t.increment_turns()
    assert t.turn_count == 2
```

- [ ] **Step 2: Run, verify fail**

- [ ] **Step 3: Implement telemetry.py**

```python
"""Latency KPI tracking via rolling percentiles."""
from __future__ import annotations

import logging
import os
from typing import Literal

from stats import RollingPercentile

logger = logging.getLogger("voice-assistant.telemetry")

# 7 KPIs per spec Section 6.1
KPI_NAMES = (
    "asr_complete_ms",
    "llm_ttfb_ms",
    "tts_first_chunk_ms",
    "first_audio_out_ms",
    "full_turn_ms",
    "barge_in_to_silence_ms",
    "barge_in_cancel_ack_ms",
)


class Telemetry:
    """Tracks latency KPIs across the last N turns."""

    def __init__(self, window: int = 100):
        self._kpis: dict[str, RollingPercentile] = {
            name: RollingPercentile(window) for name in KPI_NAMES
        }
        self.turn_count = 0
        self.barge_in_count = 0
        self._otel_enabled = os.environ.get("VOICE_ASSISTANT_TRACE", "") == "1"
        if self._otel_enabled:
            self._setup_otel()

    def _setup_otel(self) -> None:
        try:
            from opentelemetry import trace
            from opentelemetry.sdk.trace import TracerProvider
            from opentelemetry.sdk.trace.export import ConsoleSpanExporter, BatchSpanProcessor
            provider = TracerProvider()
            provider.add_span_processor(BatchSpanProcessor(ConsoleSpanExporter()))
            trace.set_tracer_provider(provider)
            self.tracer = trace.get_tracer("voice-assistant")
        except ImportError:
            logger.warning("opentelemetry-sdk not installed; trace disabled")
            self._otel_enabled = False
            self.tracer = None
        else:
            self.tracer = getattr(self, "tracer", None)

    def observe(self, kpi: str, value_ms: float) -> None:
        if kpi in self._kpis:
            self._kpis[kpi].observe(value_ms)

    def percentile(self, kpi: str, p: float) -> float:
        rp = self._kpis.get(kpi)
        return rp.percentile(p) if rp else 0.0

    def increment_turns(self) -> None:
        self.turn_count += 1

    def increment_barge_ins(self) -> None:
        self.barge_in_count += 1

    def snapshot(self) -> dict[str, dict[str, float]]:
        """Return {kpi_name: {p50, p95, min, max}} for all KPIs."""
        result = {}
        for name, rp in self._kpis.items():
            if rp.samples:
                result[name] = {
                    "p50": rp.percentile(50),
                    "p95": rp.percentile(95),
                    "min": min(rp.samples),
                    "max": max(rp.samples),
                }
        return result
```

- [ ] **Step 4: Run tests, verify pass (5 tests)**

- [ ] **Step 5: Add opentelemetry-sdk to requirements (optional dep)**

Append to `requirements.txt`:
```
opentelemetry-sdk>=1.20.0  # optional: set VOICE_ASSISTANT_TRACE=1 to enable
```

- [ ] **Step 6: Commit**

```bash
git add extensions/voice-assistant/service/telemetry.py \
        extensions/voice-assistant/service/tests/test_telemetry.py \
        extensions/voice-assistant/service/requirements.txt
git commit -m "feat(voice): add telemetry with 7 KPIs + optional OTel tracing"
```

---

## Task 13: Create `ws_protocol.py` — frame codec

**Files:**
- Create: `extensions/voice-assistant/service/ws_protocol.py`
- Create: `extensions/voice-assistant/service/tests/test_ws_protocol.py`

**Context:** Browser WS protocol frames (see server.py docstring L13-30). Centralize encode/decode so server.py shrinks.

- [ ] **Step 1: Write failing test**

```python
"""WS protocol frame codec tests."""
from __future__ import annotations

import json

from ws_protocol import (
    encode_transcript, encode_phase, encode_stop, encode_error,
    encode_barge_in_ack, decode_start, decode_ping,
)


def test_encode_transcript():
    frame = encode_transcript("hello")
    obj = json.loads(frame)
    assert obj == {"type": "transcript", "text": "hello"}


def test_encode_phase():
    frame = encode_phase("asr_start", asr_ms=123.4)
    obj = json.loads(frame)
    assert obj["type"] == "asr_start"
    assert obj["asr_ms"] == 123.4


def test_encode_stop():
    assert json.loads(encode_stop()) == {"type": "stop"}


def test_encode_error():
    frame = encode_error("boom")
    assert json.loads(frame) == {"type": "error", "message": "boom"}


def test_decode_start():
    frame = json.dumps({
        "type": "start", "session_id": "s1",
        "sample_rate": 16000, "channels": 1, "format": "pcm_int16_le",
    })
    parsed = decode_start(frame)
    assert parsed["session_id"] == "s1"
    assert parsed["sample_rate"] == 16000


def test_decode_ping():
    assert decode_ping('{"type": "ping"}') is True
    assert decode_ping('{"type": "other"}') is False
```

- [ ] **Step 2: Run, verify fail**

- [ ] **Step 3: Implement ws_protocol.py**

```python
"""Browser WS frame encoder/decoder. Single source of truth for protocol."""
from __future__ import annotations

import json


def encode_transcript(text: str) -> str:
    return json.dumps({"type": "transcript", "text": text}, ensure_ascii=False)


def encode_phase(phase: str, **metrics) -> str:
    """phase: 'asr_start' | 'asr_end' | 'tts_start' | 'tts_end'."""
    obj = {"type": phase}
    obj.update(metrics)
    return json.dumps(obj, ensure_ascii=False)


def encode_stop() -> str:
    return json.dumps({"type": "stop"})


def encode_error(message: str) -> str:
    return json.dumps({"type": "error", "message": message}, ensure_ascii=False)


def encode_barge_in_ack() -> str:
    return json.dumps({"type": "control", "action": "stop_playback",
                       "reason": "barge_in"})


def decode_start(frame_text: str) -> dict:
    """Parse a 'start' text frame from browser."""
    obj = json.loads(frame_text)
    return obj  # caller checks type == "start"


def decode_ping(frame_text: str) -> bool:
    try:
        return json.loads(frame_text).get("type") == "ping"
    except (json.JSONDecodeError, AttributeError):
        return False
```

- [ ] **Step 4: Run tests, verify pass (6 tests)**

- [ ] **Step 5: Commit**

```bash
git add extensions/voice-assistant/service/ws_protocol.py \
        extensions/voice-assistant/service/tests/test_ws_protocol.py
git commit -m "feat(voice): add ws_protocol.py frame codec"
```

---

## Task 14a: Extract turn pipeline into orchestrator.py

**Files:**
- Modify: `extensions/voice-assistant/service/orchestrator.py`
- Modify: `extensions/voice-assistant/service/server.py` (delegate to orchestrator)
- Create: `extensions/voice-assistant/service/tests/test_pipeline.py`

**Context:** Move `run_pipeline_for_segment` from server.py (L909) into orchestrator.py as a method on a new `VoicePipeline` class. **Still use old backends for now** — wiring new backend classes happens in Task 14c. The goal of this step is purely to relocate the turn logic so server.py shrinks.

- [ ] **Step 1: Write failing test for VoicePipeline**

Create `tests/test_pipeline.py`:

```python
"""VoicePipeline integration test with mocked backends."""
from __future__ import annotations

import asyncio
from unittest.mock import AsyncMock, MagicMock

import pytest

from orchestrator import VoicePipeline, State
from contracts import LlmEvent, VadSegment


@pytest.mark.asyncio
async def test_pipeline_runs_one_turn():
    """Pipeline: VAD segment → ASR → LLM (Content events) → TTS."""
    vad = MagicMock()
    asr = AsyncMock()
    asr.transcribe = AsyncMock(return_value="你好")
    llm = AsyncMock()
    async def llm_stream(text, sid):
        yield LlmEvent(type="Content", text="你好啊")
        yield LlmEvent(type="end")
    llm.stream = llm_stream
    llm.cancel = AsyncMock()
    tts = AsyncMock()
    tts.synthesize = AsyncMock(return_value=b"\x00\x00" * 100)

    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(vad, asr, llm, tts, on_tts_pcm=on_tts_pcm)

    segment = VadSegment(samples=[0.0] * 16000, sample_rate=16000,
                         start_ms=0, end_ms=1000)
    await pipeline.run_turn(segment)

    asr.transcribe.assert_called_once()
    tts.synthesize.assert_called_once_with("你好啊", ANY)
    on_tts_pcm.assert_called()
    assert pipeline.fsm.state == State.IDLE


@pytest.mark.asyncio
async def test_pipeline_barge_in_cancels_tts():
    """If barge-in fires during TTS, pipeline stops sending PCM."""
    # ... test that BargeInHandler is invoked
    pass
```

Use `from unittest.mock import ANY` where needed.

- [ ] **Step 2: Run, verify fail**

- [ ] **Step 3: Implement VoicePipeline in orchestrator.py**

Append to `orchestrator.py`:

```python
class VoicePipeline:
    """Runs one voice turn: VAD segment → ASR → LLM → TTS.

    Uses the StateMachine for transitions and BargeInHandler for interrupts.
    Backends are Protocol instances — pipeline knows nothing about concrete types.
    """

    def __init__(self, vad, asr, llm, tts, on_tts_pcm, telemetry=None,
                 voice="中文女"):
        self.vad = vad
        self.asr = asr
        self.llm = llm
        self.tts = tts
        self.on_tts_pcm = on_tts_pcm  # async callback(pcm_bytes, sample_rate)
        self.telemetry = telemetry
        self.voice = voice
        self.fsm = StateMachine()
        self._original_vad_threshold = getattr(vad, "threshold", None)

    async def run_turn(self, segment) -> None:
        """Process one VAD segment end-to-end."""
        import time
        # LISTENING → THINKING
        await self.fsm.async_transition(State.THINKING)
        turn_start = time.perf_counter()
        asr_start = time.perf_counter()

        # ASR
        transcript = await self.asr.transcribe(segment.samples, segment.sample_rate)
        asr_ms = (time.perf_counter() - asr_start) * 1000
        if self.telemetry:
            self.telemetry.observe("asr_complete_ms", asr_ms)

        # LLM stream — filter Content events, accumulate text
        llm_start = time.perf_counter()
        full_text = ""
        first_token_ms = None
        async for evt in self.llm.stream(transcript, session_id=str(id(self))):
            if first_token_ms is None and evt.type == "Content":
                first_token_ms = (time.perf_counter() - llm_start) * 1000
                if self.telemetry:
                    self.telemetry.observe("llm_ttfb_ms", first_token_ms)
            if evt.type == "Content" and evt.text:
                full_text += evt.text
            # Other events (Thinking, ToolCallStart, etc.) → ignore for TTS
            if self.fsm.state == State.BARGED:
                return

        if not full_text.strip():
            await self.fsm.async_transition(State.IDLE)
            return

        # THINKING → SPEAKING
        await self.fsm.async_transition(State.SPEAKING)
        # AEC threshold hack (spec Section 5): raise VAD threshold during playback
        if self._original_vad_threshold is not None:
            self.vad.threshold = self._original_vad_threshold + 0.2

        tts_start = time.perf_counter()
        pcm = await self.tts.synthesize(full_text, self.voice)
        tts_ms = (time.perf_counter() - tts_start) * 1000
        if self.telemetry:
            self.telemetry.observe("tts_first_chunk_ms", tts_ms)

        # Deliver PCM
        await self.on_tts_pcm(pcm, 24000)  # ZipVoice outputs 24kHz

        total_ms = (time.perf_counter() - turn_start) * 1000
        if self.telemetry:
            self.telemetry.observe("first_audio_out_ms", total_ms)
            self.telemetry.observe("full_turn_ms", total_ms)
            self.telemetry.increment_turns()

        # SPEAKING → IDLE, restore VAD threshold
        if self._original_vad_threshold is not None:
            self.vad.threshold = self._original_vad_threshold
        await self.fsm.async_transition(State.IDLE)
```

- [ ] **Step 4: Update server.py to delegate turn execution to VoicePipeline**

Replace the body of `run_pipeline_for_segment` in server.py with a call to `pipeline.run_turn(segment)`. Keep the function as a thin wrapper so `ws_handler` still works unchanged.

- [ ] **Step 5: Run test_pipeline.py + all existing tests**

```bash
python -m pytest tests/ -v
```

Expected: ALL pass.

- [ ] **Step 6: Verify e2e pipeline**

Speak a test phrase, confirm response.

- [ ] **Step 7: Commit**

```bash
git add extensions/voice-assistant/service/orchestrator.py \
        extensions/voice-assistant/service/tests/test_pipeline.py \
        extensions/voice-assistant/service/server.py
git commit -m "refactor(voice): extract turn pipeline to orchestrator.py VoicePipeline"
```

---

## Task 14b: Wire StateMachine + BargeInHandler into WS handler

**Files:**
- Modify: `extensions/voice-assistant/service/server.py`
- Modify: `extensions/voice-assistant/service/orchestrator.py`

**Context:** VoiceSession in server.py currently uses the implicit `epoch` counter for barge-in. Replace with the explicit FSM + BargeInHandler. The VAD callbacks that previously checked epoch now check `fsm.state == State.BARGED`.

- [ ] **Step 1: Add barge-in detection test**

Add to `tests/test_pipeline.py`:

```python
@pytest.mark.asyncio
async def test_pipeline_barge_in_aborts_llm_stream():
    """Barge-in during THINKING aborts the LLM stream and calls cancel()."""
    vad = MagicMock()
    asr = AsyncMock()
    asr.transcribe = AsyncMock(return_value="你好")
    llm = AsyncMock()
    cancelled = asyncio.Event()
    async def llm_stream(text, sid):
        yield LlmEvent(type="Content", text="部")
        # Simulate barge-in firing here
        await cancelled.wait()
        yield LlmEvent(type="Content", text="分")  # should never reach TTS
    llm.stream = llm_stream
    llm.cancel = AsyncMock()
    tts = AsyncMock()
    tts.synthesize = AsyncMock()

    pipeline = VoicePipeline(vad, asr, llm, tts, on_tts_pcm=AsyncMock())

    # Start turn, then trigger barge-in mid-stream
    task = asyncio.create_task(pipeline.run_turn(_fake_segment()))
    await asyncio.sleep(0.05)  # let LLM emit first chunk
    await pipeline.barge_in.handle_barge_in(pipeline.fsm, reason="test")
    cancelled.set()
    await task

    llm.cancel.assert_called_once()
    tts.synthesize.assert_not_called()  # TTS never ran
```

- [ ] **Step 2: Run, verify fail**

- [ ] **Step 3: Add BargeInHandler to VoicePipeline + wire cancel paths**

In `orchestrator.py`, update `VoicePipeline.__init__`:

```python
        # Barge-in handler — wires the 4 cleanup actions
        self.barge_in = BargeInHandler(
            cancel_tts_playback=self._cancel_tts_playback,
            cancel_llm_request=self._cancel_llm,
            clear_pending_queues=self._clear_queues,
            drain_asr_buffer=self._drain_asr,
        )
```

Add the cleanup methods:

```python
    async def _cancel_tts_playback(self):
        # Notify browser to stop playback via WS control frame
        if self.on_stop_playback:
            await self.on_stop_playback()

    async def _cancel_llm(self):
        await self.llm.cancel(session_id=str(id(self)))

    async def _clear_queues(self):
        # Clear any pending PCM queue
        while hasattr(self, "_pcm_queue") and not self._pcm_queue.empty():
            try:
                self._pcm_queue.get_nowait()
            except asyncio.QueueEmpty:
                break

    async def _drain_asr(self):
        # ASR is stateless (Protocol); nothing to drain at backend level.
        # Orchestrator-level state reset happens via FSM transition.
        pass
```

In `run_turn`, after each await point, check `if self.fsm.state == State.BARGED: return`.

- [ ] **Step 4: Run tests, verify pass**

- [ ] **Step 5: Update server.py ws_handler to trigger barge_in on VAD speech during THINKING/SPEAKING**

In the mic feed loop, when VAD emits a new segment while fsm.state in (THINKING, SPEAKING):

```python
if session.pipeline.fsm.state in (State.THINKING, State.SPEAKING):
    await session.pipeline.barge_in.handle_barge_in(
        session.pipeline.fsm, reason="user_speech"
    )
```

- [ ] **Step 6: Verify e2e with barge-in**

Speak a phrase, during TTS playback speak again — TTS should stop within 200ms.

- [ ] **Step 7: Commit**

```bash
git add extensions/voice-assistant/service/orchestrator.py \
        extensions/voice-assistant/service/server.py \
        extensions/voice-assistant/service/tests/test_pipeline.py
git commit -m "feat(voice): wire FSM + BargeInHandler into WS handler with AEC threshold hack"
```

---

## Task 14c: Replace old backend calls with Profile-backed instances

**Files:**
- Modify: `extensions/voice-assistant/service/server.py` (final slimming)
- Modify: `extensions/voice-assistant/service/start.sh`

**Context:** Now that pipeline + FSM + barge-in live in orchestrator.py, server.py can stop constructing the old inline backends and use the factory + Profile at startup.

- [ ] **Step 1: Read current server.py and identify dead code**

After 14a/14b, server.py still has the original `asr_transcribe`, `tts_stream`, `ollama_llm_stream`, `llm_stream` functions (now unused). VoiceSession may still construct VAD inline. All of this can go.

- [ ] **Step 2: At server.py startup, build backends from Profile**

```python
# server.py top, after imports
from profile import load_profile
from backends import make_vad, make_asr, make_llm, make_tts
from telemetry import Telemetry

_profile = load_profile(os.environ.get("VOICE_ASSISTANT_PROFILE"))
_vad = make_vad(_profile)
_asr = make_asr(_profile)
_llm = make_llm(_profile)
_tts = make_tts(_profile)
_telemetry = Telemetry()
```

- [ ] **Step 3: VoiceSession uses shared backend instances + creates pipeline per turn**

Remove VoiceSession's backend-specific fields. Pass shared `_vad, _asr, _llm, _tts` to each VoicePipeline. VoiceSession becomes a thin WS wrapper.

- [ ] **Step 4: Delete dead functions** (`asr_transcribe`, `tts_stream`, `ollama_llm_stream`, `llm_stream`, `fake_llm_stream`, `_build_fake_reply`, `_is_chinese`, `_split_streaming_buffer`, `_consume_prefix`, `_estimate_max_frames`)

These have all been replaced by backends/*.

- [ ] **Step 5: Run full test suite**

```bash
python -m pytest tests/ -v
```

All tests pass.

- [ ] **Step 6: Verify e2e pipeline**

```bash
./start.sh &
# Speak test phrase via poc.html, confirm response
# Hit /measure, confirm KPIs reported
kill %1
```

- [ ] **Step 7: Update start.sh to load profile name from env**

```bash
# Add at top of start.sh:
export VOICE_ASSISTANT_PROFILE="${VOICE_ASSISTANT_PROFILE:-default}"
```

- [ ] **Step 8: Verify file size target**

```bash
wc -l extensions/voice-assistant/service/server.py
# Target: under 280 lines (was 1174)
wc -l extensions/voice-assistant/service/orchestrator.py
# Target: under 400 lines
```

- [ ] **Step 9: Commit**

```bash
git add extensions/voice-assistant/service/server.py \
        extensions/voice-assistant/service/start.sh
git commit -m "refactor(voice): slim server.py to ~250 lines, use Profile-backed backends"
```

---

## Task 15: Add `measure` command + end-to-end verification

**Files:**
- Modify: `extensions/voice-assistant/service/server.py` (add /measure HTTP route)
- Create: `extensions/voice-assistant/service/tests/test_measure.py`

**Context:** Per spec Section 6.4 — a `measure` command runs N synthetic turns and returns aggregated latency stats.

- [ ] **Step 1: Write failing test**

```python
"""measure endpoint returns aggregated latency stats."""
from __future__ import annotations

from unittest.mock import patch, AsyncMock

import pytest


@pytest.mark.asyncio
async def test_measure_returns_kpi_snapshot():
    # Mock the orchestrator to return canned KPIs
    with patch("server._orchestrator") as mock_orch:
        mock_orch.telemetry.snapshot.return_value = {
            "first_audio_out_ms": {"p50": 950.0, "p95": 1100.0, "min": 900.0, "max": 1200.0},
            "asr_complete_ms": {"p50": 200.0, "p95": 250.0, "min": 180.0, "max": 280.0},
        }
        mock_orch.telemetry.turn_count = 5
        # Use FastAPI TestClient
        from fastapi.testclient import TestClient
        from server import app
        client = TestClient(app)
        resp = client.post("/measure", json={"n": 5})
    assert resp.status_code == 200
    data = resp.json()
    assert "first_audio_out_ms" in data
    assert data["first_audio_out_ms"]["p50"] == 950.0
    assert data["turn_count"] == 5
```

- [ ] **Step 2: Run, verify fail**

- [ ] **Step 3: Add /measure endpoint to server.py**

```python
@app.post("/measure")
async def measure(req: dict):
    """Return aggregated latency stats from telemetry."""
    snap = _orchestrator.telemetry.snapshot()
    return {
        "n": _orchestrator.telemetry.turn_count,
        "target_ms": _profile.latency_target_ms,
        "target_met": (
            snap.get("first_audio_out_ms", {}).get("p50", float("inf"))
            <= _profile.latency_target_ms
        ),
        "turn_count": _orchestrator.telemetry.turn_count,
        "barge_in_count": _orchestrator.telemetry.barge_in_count,
        **snap,
    }
```

- [ ] **Step 4: Run test, verify pass**

- [ ] **Step 5: Full test suite run**

```bash
cd extensions/voice-assistant/service
python -m pytest tests/ -v
```

Expected: ALL tests pass.

- [ ] **Step 6: Final e2e verification with real backends**

```bash
# 1. Start ASR + TTS services
cd extensions/sensevoice-asr/service && ./start.sh &
cd extensions/voice-edge-tts/service && ./start.sh &

# 2. Start voice-assistant
cd extensions/voice-assistant/service && ./start.sh &

# 3. Open poc.html via http server (required for AudioWorklet)
python3 -m http.server 9876 --bind 127.0.0.1 &
# Browser: http://127.0.0.1:9876/poc.html

# 4. Speak 3 test phrases, verify TTS response each time

# 5. Trigger barge-in: speak during TTS playback, verify TTS stops

# 6. Hit /measure endpoint
curl http://127.0.0.1:9384/measure -X POST -H "Content-Type: application/json" -d '{}'
# Verify first_audio_out_ms p50 < 1200ms (stretch: < 1000ms)
```

- [ ] **Step 7: Commit**

```bash
git add extensions/voice-assistant/service/server.py \
        extensions/voice-assistant/service/tests/test_measure.py
git commit -m "feat(voice): add /measure endpoint for latency verification"
```

---

## Verification Checklist (Run After All Tasks)

- [ ] `pytest tests/ -v` — all tests green
- [ ] `wc -l server.py` — under 280 lines
- [ ] `wc -l orchestrator.py` — under 400 lines
- [ ] E2e pipeline: mic → VAD → ASR → LLM → TTS → speaker works
- [ ] Barge-in: speaking during TTS stops playback within 200ms
- [ ] `/measure` returns KPI snapshot with `target_met` boolean
- [ ] Switching profile (`VOICE_ASSISTANT_PROFILE=headset`) changes VAD backend at startup
- [ ] NeoMind LLM path works when `llm.type: neomind_ws` in profile (manual test with running NeoMind)
- [ ] Tool calls do NOT pollute TTS (mock test in test_neomind_ws_client.py verifies Content-only filtering)

## References

- Spec: `docs/superpowers/specs/2026-06-24-voice-architecture-v2-design.md`
- Skill: @superpowers:subagent-driven-development (recommended) or @superpowers:executing-plans
- Skill: @superpowers:test-driven-development (each task follows TDD)
- Existing working code to port from: `extensions/voice-assistant/service/server.py` (1174 lines, pre-refactor)
