# Voice Assistant Greeting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a pre-synthesized "greeting" audio clip that plays immediately on session start so the user doesn't hear silence after clicking connect.

**Architecture:** Greeting text lives in profile YAML as `interaction.greeting_text` (default empty = disabled). At server startup, `_warm_greeting()` synthesizes the text via the existing TTS backend and caches the 16kHz mono PCM in a module-level `_GREETING_PCM` (third sibling to `_ACK_PCM_BANK` / `_STAGE_FILLER_BANK`). On `{"type":"start"}`, `ws_handler` synchronously emits a `{"type":"greeting","text":...}` subtitle frame followed by the binary PCM. Barge-in during greeting flushes the browser queue immediately via a new guard at the top of the `pcm_complete is not None` branch.

**Tech Stack:** Python 3.12 / FastAPI / pytest / vanilla JS (poc.html). All changes inside `extensions/voice-assistant/`.

**Spec:** `docs/superpowers/specs/2026-06-27-voice-greeting-design.md` — read this for rationale on every design choice below.

**Working dir for all commands:** `extensions/voice-assistant/service/`

---

## File Structure

| File | Change |
|------|--------|
| `profile.py` | Add `greeting_text: str = ""` field + one line in `from_dict` |
| `profiles/default.yaml` | Add `interaction.greeting_text: ""` (default disabled) |
| `ws_protocol.py` | Add `encode_greeting(text)` function |
| `server.py` | Add `_GREETING_PCM` cache + `_warm_greeting()` + wire into `_warm_banks_async()` + add `VoiceSession.greeting_active` + greeting push in `ws_handler` start branch + barge-in flush guard in `pcm_complete` branch |
| `measure_common.py` | Recognize `greeting` event, add `greeting_pcm_chunks` counter |
| `poc.html` | Add `case 'greeting':` branch (subtitle + log) |
| `tests/test_greeting.py` (new) | Unit tests for profile field + `_warm_greeting` |
| `tests/test_ws_integration.py` | Integration tests for greeting push + barge-in flush |
| `tests/test_measure.py` (or new) | Unit test for `measure_common.run_one_turn` greeting isolation |
| `README.md` | Add "Greeting (Say-First)" section |

---

## Task 1: Add `greeting_text` to Profile

**Files:**
- Modify: `extensions/voice-assistant/service/profile.py:11-25` (Profile dataclass) and `profile.py:44-62` (`from_dict`)
- Modify: `extensions/voice-assistant/service/profiles/default.yaml:26-33` (interaction block)
- Test: `extensions/voice-assistant/service/tests/test_greeting.py` (new file)

- [ ] **Step 1: Write the failing test**

Create `tests/test_greeting.py`:

```python
"""Unit tests for the greeting (say-first) feature."""
from __future__ import annotations

from profile import Profile


def test_greeting_text_defaults_to_empty():
    """Profile without greeting_text in YAML defaults to empty string."""
    p = Profile.from_dict({})
    assert p.greeting_text == ""


def test_greeting_text_loaded_from_interaction_dict():
    """Profile reads greeting_text from interaction.* block."""
    p = Profile.from_dict({"interaction": {"greeting_text": "你好"}})
    assert p.greeting_text == "你好"


def test_greeting_text_whitespace_preserved():
    """Whitespace-only greeting_text is preserved as-is (empty check happens
    in _warm_greeting via .strip(), not in Profile.from_dict)."""
    p = Profile.from_dict({"interaction": {"greeting_text": "  hi  "}})
    assert p.greeting_text == "  hi  "
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_greeting.py -v`
Expected: FAIL with `AttributeError: 'Profile' object has no attribute 'greeting_text'` (or `TypeError: __init__() got an unexpected keyword argument 'greeting_text'`)

- [ ] **Step 3: Add field to Profile dataclass**

In `profile.py`, add `greeting_text: str` to the dataclass field list (after `stage_filler_words`):

```python
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
    barge_in_ack: bool
    ack_words: list[str]
    stage_filler_words: dict[str, list[str]]
    greeting_text: str  # NEW
```

- [ ] **Step 4: Read field in `from_dict`**

In `profile.py` `from_dict`, add one line inside the `return cls(...)` call (after `stage_filler_words=stage_filler_words,`):

```python
        stage_filler_words=stage_filler_words,
        greeting_text=interaction.get("greeting_text", ""),  # NEW
    )
```

- [ ] **Step 5: Run test to verify it passes**

Run: `python -m pytest tests/test_greeting.py -v`
Expected: PASS (3 tests)

- [ ] **Step 6: Add to default.yaml**

In `profiles/default.yaml`, add `greeting_text: ""` to the `interaction:` block (after `stage_filler_words`):

```yaml
interaction:
  barge_in: full
  latency_target_ms: 1200
  barge_in_ack: true
  ack_words: ["好的", "嗯哼", "明白"]
  stage_filler_words:
    thinking: ["让我想想", "嗯,让我想想", "稍等一下"]
    tool_call: ["我查一下", "搜索中", "查一下资料"]
  greeting_text: ""  # NEW — empty = disabled (opt-in due to AEC gap; see spec)
```

- [ ] **Step 7: Verify default profile loads with empty greeting**

Run: `python -c "from profile import load_profile; p = load_profile('default'); print(repr(p.greeting_text))"`
Expected: prints `''`

- [ ] **Step 8: Commit**

```bash
cd /Users/shenmingming/CamThink\ Project/NeoMind-Extensions
git add extensions/voice-assistant/service/profile.py \
        extensions/voice-assistant/service/profiles/default.yaml \
        extensions/voice-assistant/service/tests/test_greeting.py
git commit -m "feat(voice): add greeting_text field to Profile

Default empty (greeting disabled). Opt-in via profile YAML
interaction.greeting_text. See spec for AEC rationale."
```

---

## Task 2: Add `encode_greeting` to ws_protocol

**Files:**
- Modify: `extensions/voice-assistant/service/ws_protocol.py` (add after `encode_llm_sentence` around line 41)
- Test: `extensions/voice-assistant/service/tests/test_ws_protocol.py` (add to existing)

- [ ] **Step 1: Write the failing test**

Add to `tests/test_ws_protocol.py`:

```python
def test_encode_greeting():
    """Greeting frame carries text for browser subtitle rendering."""
    import json
    frame = encode_greeting("你好")
    obj = json.loads(frame)
    assert obj["type"] == "greeting"
    assert obj["text"] == "你好"
```

Also add `encode_greeting` to the import line at the top of the test file (or to whatever import style the file uses — check the existing imports first).

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_ws_protocol.py::test_encode_greeting -v`
Expected: FAIL with `NameError: name 'encode_greeting' is not defined`

- [ ] **Step 3: Add encoder to ws_protocol.py**

In `ws_protocol.py`, add after `encode_llm_sentence`:

```python
def encode_greeting(text: str) -> str:
    """Greeting (say-first) frame — emitted once on session start,
    followed by the pre-synthesized greeting PCM as binary frames.

    Optional frame: clients without handling for ``greeting`` simply
    ignore the text frame; the binary PCM still plays via the standard
    playback queue. No ``tts_start``/``tts_end`` is emitted around
    greeting (those mark turn lifecycle only).
    """
    return json.dumps(
        {"type": "greeting", "text": text},
        ensure_ascii=False,
    )
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_ws_protocol.py::test_encode_greeting -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add extensions/voice-assistant/service/ws_protocol.py \
        extensions/voice-assistant/service/tests/test_ws_protocol.py
git commit -m "feat(voice): add encode_greeting to ws_protocol

Subtitle companion frame for greeting PCM. No tts_start/tts_end —
greeting is not a turn."
```

---

## Task 3: Add `_GREETING_PCM` cache + `_warm_greeting` + wire into `_warm_banks_async`

**Files:**
- Modify: `extensions/voice-assistant/service/server.py:140-143` (cache vars near other banks), `server.py:162-202` (`_warm_banks_async`), and add new `_warm_greeting` function
- Test: `extensions/voice-assistant/service/tests/test_greeting.py` (append)

- [ ] **Step 1: Append failing tests to test_greeting.py**

```python
import asyncio
from unittest.mock import AsyncMock, MagicMock


def _make_server_with_mock_tts(monkeypatch, synth_return: bytes | None):
    """Helper: import server fresh, mock _tts_backend.synthesize."""
    import server
    mock_tts = MagicMock()
    if synth_return is not None:
        mock_tts.synthesize = AsyncMock(return_value=synth_return)
    else:
        mock_tts.synthesize = AsyncMock(side_effect=RuntimeError("tts down"))
    monkeypatch.setattr(server, "_tts_backend", mock_tts)
    return server


def test_warm_greeting_noop_when_text_empty(monkeypatch):
    """Empty greeting_text -> _GREETING_PCM stays None, no TTS call."""
    server = _make_server_with_mock_tts(monkeypatch, b"\x10\x00" * 100)
    monkeypatch.setattr(server, "_GREETING_PCM", None)
    monkeypatch.setattr(server._profile, "greeting_text", "")
    asyncio.run(server._warm_greeting())
    assert server._GREETING_PCM is None
    server._tts_backend.synthesize.assert_not_called()


def test_warm_greeting_synthesizes_when_text_set(monkeypatch):
    """Non-empty greeting_text -> _GREETING_PCM populated with browser PCM."""
    server = _make_server_with_mock_tts(monkeypatch, b"\x10\x00" * 100)
    monkeypatch.setattr(server, "_GREETING_PCM", None)
    monkeypatch.setattr(server, "TTS_VOICE", "zh")
    monkeypatch.setattr(server._profile, "greeting_text", "你好")
    asyncio.run(server._warm_greeting())
    assert server._GREETING_PCM is not None
    assert isinstance(server._GREETING_PCM, bytes)
    assert len(server._GREETING_PCM) > 0
    server._tts_backend.synthesize.assert_called_once_with("你好", "zh")


def test_warm_greeting_swallows_tts_failure(monkeypatch):
    """TTS failure -> _GREETING_PCM stays None (greeting silently disabled)."""
    server = _make_server_with_mock_tts(monkeypatch, None)
    monkeypatch.setattr(server, "_GREETING_PCM", None)
    monkeypatch.setattr(server, "TTS_VOICE", "zh")
    monkeypatch.setattr(server._profile, "greeting_text", "你好")
    asyncio.run(server._warm_greeting())  # must not raise
    assert server._GREETING_PCM is None
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python -m pytest tests/test_greeting.py -v`
Expected: 3 new tests FAIL with `AttributeError: module 'server' has no attribute '_warm_greeting'`

- [ ] **Step 3: Add module-level cache var**

In `server.py`, after line 143 (`_STAGE_BANK_WARMED = False`), add:

```python
# Greeting (say-first) — single PCM clip, or None if disabled / synth failed.
# Mirrors _ACK_PCM_BANK pattern but holds one clip, not a list.
_GREETING_PCM: bytes | None = None
```

- [ ] **Step 4: Add `_warm_greeting` function**

In `server.py`, after `_warm_banks_async` (around line 202), add:

```python
async def _warm_greeting() -> None:
    """Pre-synthesize greeting clip once at startup.

    No-op if profile's greeting_text is empty/whitespace (greeting disabled).
    On TTS failure, leaves _GREETING_PCM as None — greeting silently
    disabled, server still starts.
    """
    global _GREETING_PCM
    text = (_profile.greeting_text or "").strip()
    if not text:
        return
    try:
        raw_pcm = await _tts_backend.synthesize(text, TTS_VOICE)
        # _tts_backend.synthesize returns 24kHz mono raw PCM; resample to
        # 16kHz mono int16 LE for the browser (same path as ack/filler banks).
        _GREETING_PCM = _tts_to_browser_pcm(raw_pcm, 24000, 1)
        logger.info("greeting clip warmed: %d bytes (%d samples)",
                    len(_GREETING_PCM), len(_GREETING_PCM) // 2)
    except Exception as e:
        logger.warning("greeting synth failed — greeting disabled: %s", e)
        _GREETING_PCM = None
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `python -m pytest tests/test_greeting.py -v`
Expected: 6 PASS (3 from Task 1 + 3 new)

- [ ] **Step 6: Commit**

```bash
git add extensions/voice-assistant/service/server.py \
        extensions/voice-assistant/service/tests/test_greeting.py
git commit -m "feat(voice): add _GREETING_PCM cache + _warm_greeting helper

Synthesizes greeting_text once via existing TTS backend, caches 16kHz
mono PCM. No-op when text empty. Silently disables on TTS failure."
```

---

## Task 4: Wire `_warm_greeting` into `_warm_banks_async`

**Files:**
- Modify: `extensions/voice-assistant/service/server.py:162-202` (`_warm_banks_async` body, append greeting block at the end)
- Test: existing tests in `test_greeting.py` cover the function in isolation; this task is pure wiring. Manual verification via the lifespan startup smoke test below.

- [ ] **Step 1: Append greeting warmup to `_warm_banks_async`**

In `_warm_banks_async` in `server.py`, after the stage filler block (after line 201), add:

```python
    # ---- greeting clip ----
    # No WARMED flag needed: _warm_greeting is idempotent — empty greeting_text
    # short-circuits, non-empty re-synthesizes harmlessly if called twice.
    await _warm_greeting()
```

- [ ] **Step 2: Smoke-test the wired startup path**

Run:
```bash
cd extensions/voice-assistant/service
VOICE_ASSISTANT_VAD_BACKEND=energy python -c "
import asyncio, server
async def go():
    # Force non-empty greeting for the smoke test
    server._profile.greeting_text = '你好'
    await server._warm_banks_async()
    print('GREETING_PCM:', type(server._GREETING_PCM).__name__,
          'len=' + str(len(server._GREETING_PCM)) if server._GREETING_PCM else 'None')
asyncio.run(go())
"
```

Expected: prints `GREETING_PCM: bytes len=<some positive number>` (or `GREETING_PCM: NoneType None` if TTS backend isn't running locally — both are acceptable; the function shouldn't crash either way).

- [ ] **Step 3: Commit**

```bash
git add extensions/voice-assistant/service/server.py
git commit -m "feat(voice): wire greeting warmup into _warm_banks_async

Greeting clip is now pre-synthesized alongside ack + stage-filler banks
at server startup (lifespan event)."
```

---

## Task 5: Add `greeting_active` flag to `VoiceSession`

**Files:**
- Modify: `extensions/voice-assistant/service/server.py:337-382` (VoiceSession `__init__`)
- Test: covered by Task 7's integration tests that assert the flag exists and toggles.

- [ ] **Step 1: Add the flag to `VoiceSession.__init__`**

In `server.py` `VoiceSession.__init__`, after `self.tts_last_chunk_ts: float | None = None` (or wherever `tts_active` is initialized — find it first), add:

```python
        # Greeting (say-first) — True between greeting push and the first
        # user-speech detection that ends the greeting window. Used by
        # ws_handler's pcm_complete branch to emit barge_in immediately
        # (before the new turn starts) so the browser flushes the greeting
        # queue without waiting for the new turn's first TTS PCM.
        self.greeting_active = False
```

**Locate `tts_active` first** with: `grep -n "tts_active" extensions/voice-assistant/service/server.py | head` — add `greeting_active` near it for cohesion.

- [ ] **Step 2: Verify VoiceSession still constructs cleanly**

Run: `python -c "import server; s = server.VoiceSession(ws=None, session_id='x'); print(s.greeting_active)"`
Expected: prints `False`

- [ ] **Step 3: Commit**

```bash
git add extensions/voice-assistant/service/server.py
git commit -m "feat(voice): add greeting_active flag to VoiceSession

Will gate the barge-in-on-first-speech flush in the next task."
```

---

## Task 6: Push greeting in `ws_handler` start branch

**Files:**
- Modify: `extensions/voice-assistant/service/server.py:782-792` (the `elif mtype == "start":` block)
- Test: `extensions/voice-assistant/service/tests/test_ws_integration.py` (append)

- [ ] **Step 1: Write the failing integration test**

Add to `tests/test_ws_integration.py`. Reuses the existing `_drain_ws` helper (lines 91-124) which handles Starlette TestClient's no-arg `receive()` correctly:

```python
def test_start_emits_greeting_when_enabled(monkeypatch):
    """When _GREETING_PCM is populated, start frame triggers
    ready -> greeting -> binary PCM, in that order. Also asserts the
    session state required for AEC echo-window correctness:
    sess.tts_active is True after the greeting push."""
    import server

    # Force greeting enabled
    fake_pcm = b"\x01\x02\x03\x04" * 10
    monkeypatch.setattr(server, "_GREETING_PCM", fake_pcm)
    monkeypatch.setattr(server._profile, "greeting_text", "hello greeting")

    # Reuse the ws_app fixture's mock backends by re-applying key patches
    monkeypatch.setattr(server, "VAD_BACKEND", "energy")
    monkeypatch.setattr(server, "VAD_ENERGY_THRESHOLD", 0.001)
    monkeypatch.setattr(server, "VAD_MIN_SPEECH_MS", 90)
    monkeypatch.setattr(server, "VAD_SILENCE_MS", 150)
    monkeypatch.setattr(server._profile, "barge_in_ack", False)
    monkeypatch.setattr(server._profile, "stage_filler_words", {})

    # Mock ASR + TTS so a downstream turn (if any frame triggers it) won't crash
    from unittest.mock import AsyncMock, MagicMock
    from contracts import TtsChunk
    mock_asr = MagicMock(); mock_asr.transcribe = AsyncMock(return_value="hi")
    monkeypatch.setattr(server, "_asr_backend", mock_asr)
    mock_tts = MagicMock()
    mock_tts.synthesize = AsyncMock(return_value=b"\x10\x00" * 100)
    async def _stream(text, voice):
        yield TtsChunk(pcm_int16=b"\x10\x00" * 100, sample_rate=24000, is_final=True)
    mock_tts.stream = _stream
    monkeypatch.setattr(server, "_tts_backend", mock_tts)

    from fastapi.testclient import TestClient
    client = TestClient(server.app)

    captured_sessions: list = []

    with client.websocket_connect("/ws?session_id=test-greet") as ws:
        ws.send_json({"type": "start", "sample_rate": 16000})
        text_frames, binary_frames = _drain_ws(
            ws, expected_types={"ready", "greeting"}, timeout_s=3.0)
        # The server's greeting push sends text frame THEN binary frame
        # synchronously. By the time _drain_ws returns (having seen both
        # ready+greeting text types), the binary may still be in flight.
        # Drain briefly so the WS context exit doesn't race the server's
        # send_binary (which would log a WebSocketDisconnect).
        import time as _t
        _deadline = _t.monotonic() + 0.5
        while _t.monotonic() < _deadline:
            try:
                extra = ws.receive()
            except Exception:
                break
            if "bytes" in extra and extra["bytes"] is not None:
                binary_frames.append(extra["bytes"])
            elif extra.get("type") == "websocket.disconnect":
                break

    # Ordering assertions
    types = [f.get("type") for f in text_frames]
    assert "ready" in types
    assert "greeting" in types
    assert types.index("ready") < types.index("greeting")
    # Greeting text payload
    greeting_frame = next(f for f in text_frames if f.get("type") == "greeting")
    assert greeting_frame["text"] == "hello greeting"
    # At least the greeting binary was pushed
    assert len(binary_frames) >= 1


def test_start_skips_greeting_when_disabled(monkeypatch):
    """When _GREETING_PCM is None (greeting_text empty), start frame
    triggers ready only — no greeting frame."""
    import server
    monkeypatch.setattr(server, "_GREETING_PCM", None)
    monkeypatch.setattr(server, "VAD_BACKEND", "energy")
    monkeypatch.setattr(server, "VAD_ENERGY_THRESHOLD", 0.001)
    monkeypatch.setattr(server, "VAD_MIN_SPEECH_MS", 90)
    monkeypatch.setattr(server, "VAD_SILENCE_MS", 150)
    monkeypatch.setattr(server._profile, "barge_in_ack", False)
    monkeypatch.setattr(server._profile, "stage_filler_words", {})

    from fastapi.testclient import TestClient
    client = TestClient(server.app)

    with client.websocket_connect("/ws?session_id=test-no-greet") as ws:
        ws.send_json({"type": "start", "sample_rate": 16000})
        # Drain briefly — we expect ready and nothing else of interest.
        # Send ping right after ready to give the drain something to terminate on.
        ws.send_json({"type": "ping"})
        text_frames, _ = _drain_ws(
            ws, expected_types={"ready", "pong"}, timeout_s=3.0)

    types = [f.get("type") for f in text_frames]
    assert "ready" in types
    assert "greeting" not in types
    assert "pong" in types
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python -m pytest tests/test_ws_integration.py::test_start_emits_greeting_when_enabled tests/test_ws_integration.py::test_start_skips_greeting_when_disabled -v`
Expected: FAIL (test_start_emits_greeting gets ready frame but no greeting frame; or timeout waiting for greeting)

- [ ] **Step 3: Implement greeting push in the start branch**

In `server.py` `ws_handler`, modify the `elif mtype == "start":` block (lines 782-792). After the existing `ready` send, add the greeting push:

```python
            elif mtype == "start":
                await sess.send_json({
                    "type": "ready",
                    "session_id": sess.session_id,
                    "asr_url": ASR_URL,
                    "tts_url": TTS_URL,
                    "voice": TTS_VOICE,
                    "vad_silence_ms": VAD_SILENCE_MS,
                    "vad_min_speech_ms": VAD_MIN_SPEECH_MS,
                    "vad_energy_threshold": VAD_ENERGY_THRESHOLD,
                })
                # Greeting (say-first): push pre-synthesized clip immediately
                # after ready. MUST be synchronous (not asyncio.create_task)
                # — measure_common attributes any binary received before the
                # first asr_start to the greeting window. See design spec.
                if _GREETING_PCM is not None:
                    from ws_protocol import encode_greeting
                    sess.greeting_active = True
                    sess.tts_active = True
                    await sess.ws.send_text(encode_greeting(_profile.greeting_text))
                    await sess.send_binary(_GREETING_PCM)
                    sess.bytes_out += len(_GREETING_PCM)
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `python -m pytest tests/test_ws_integration.py::test_start_emits_greeting_when_enabled tests/test_ws_integration.py::test_start_skips_greeting_when_disabled -v`
Expected: PASS

- [ ] **Step 5: Run full integration test suite for regression**

Run: `python -m pytest tests/test_ws_integration.py -v`
Expected: all PASS (existing tests still work — they don't set `_GREETING_PCM`, so the new branch is skipped)

- [ ] **Step 6: Commit**

```bash
git add extensions/voice-assistant/service/server.py \
        extensions/voice-assistant/service/tests/test_ws_integration.py
git commit -m "feat(voice): push greeting clip + subtitle frame on session start

Emits {type:greeting,text:...} text frame then binary PCM right after
the ready frame. Synchronous push — measurement depends on ordering."
```

---

## Task 7: Barge-in flush guard in `pcm_complete` branch

**Files:**
- Modify: `extensions/voice-assistant/service/server.py:748-770` (the `pcm_complete is not None` branch)
- Test: `extensions/voice-assistant/service/tests/test_ws_integration.py` (append)

- [ ] **Step 1: Write the failing test**

Add to `tests/test_ws_integration.py`:

```python
def test_speech_during_greeting_emits_barge_in_immediately(monkeypatch):
    """When greeting_active=True and user speech is detected (pcm_complete
    non-None), the ws_handler emits {type:barge_in} BEFORE starting the new
    turn — so the browser can flush the greeting queue without waiting
    for the new turn's first TTS PCM (which arrives 200-500ms later)."""
    import server
    import json as _json

    fake_greeting = b"\x01\x02\x03\x04" * 1000
    monkeypatch.setattr(server, "_GREETING_PCM", fake_greeting)
    monkeypatch.setattr(server._profile, "greeting_text", "hello")

    # Mock ASR returns fast so the new turn starts deterministically
    from unittest.mock import AsyncMock, MagicMock
    mock_asr = MagicMock()
    mock_asr.transcribe = AsyncMock(return_value="user speech")
    monkeypatch.setattr(server, "_asr_backend", mock_asr)

    # Mock TTS — greeting synth (called in _warm_greeting, but we already
    # have _GREETING_PCM set, so it won't be called) + turn TTS stream
    from contracts import TtsChunk
    mock_tts = MagicMock()
    mock_tts.synthesize = AsyncMock(return_value=b"\x10\x00" * 100)

    async def _tts_stream(text, voice):
        yield TtsChunk(pcm_int16=b"\x10\x00" * 100, sample_rate=24000, is_final=True)
    mock_tts.stream = _tts_stream
    monkeypatch.setattr(server, "_tts_backend", mock_tts)

    monkeypatch.setattr(server, "VAD_BACKEND", "energy")
    monkeypatch.setattr(server, "VAD_ENERGY_THRESHOLD", 0.001)
    monkeypatch.setattr(server, "VAD_MIN_SPEECH_MS", 90)
    monkeypatch.setattr(server, "VAD_SILENCE_MS", 150)
    # CRITICAL: greeting push sets sess.tts_active=True, which arms the
    # AEC echo window (_aec_active_now() returns True while tts_active
    # and within AEC_TAIL_MS). AEC boosts the effective VAD threshold by
    # AEC_ENERGY_BOOST, suppressing our test audio below detection —
    # VAD never fires, pcm_complete stays None, the barge-in guard never
    # runs, and the test deadlocks. Disable AEC for this test.
    monkeypatch.setattr(server, "AEC_MODE", "none")
    monkeypatch.setattr(server._profile, "barge_in_ack", False)
    monkeypatch.setattr(server._profile, "stage_filler_words", {})

    from fastapi.testclient import TestClient
    import numpy as np
    client = TestClient(server.app)

    # Loud PCM chunk that triggers energy VAD
    loud = (np.ones(480, dtype=np.float32) * 0.5).astype("<i2").tobytes()

    with client.websocket_connect("/ws?session_id=test-greet-bargein") as ws:
        ws.send_json({"type": "start", "sample_rate": 16000})

        # Drain greeting frames first (ready, greeting, binary PCM) so the
        # receive buffer is clean before we trigger speech. _drain_ws is
        # the existing helper that handles Starlette's no-arg receive().
        _drain_ws(ws, expected_types={"greeting"}, timeout_s=3.0)

        # Now send enough loud audio + silence to trigger VAD speech-end.
        # 10 chunks of 480-sample loud audio (0.5 amp → well above 0.001
        # threshold) followed by 20 chunks of silence.
        for _ in range(10):
            ws.send_bytes(loud)
        silence = b"\x00\x00" * 480
        for _ in range(20):
            ws.send_bytes(silence)

        # Drain frames — barge_in MUST appear. _drain_ws terminates when
        # the expected type is seen OR timeout. Starlette TestClient's
        # ws.receive() blocks on the underlying socket; if the server
        # doesn't emit, we rely on the timeout_s deadline in _drain_ws.
        text_frames, _ = _drain_ws(
            ws, expected_types={"barge_in"}, timeout_s=5.0)

    types = [f.get("type") for f in text_frames]
    assert "barge_in" in types, (
        "Expected {type:barge_in} frame after user speech during greeting, "
        f"got text frames: {types}")
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_ws_integration.py::test_speech_during_greeting_emits_barge_in_immediately -v`
Expected: FAIL with assertion error (`seen_barge_in` is False) — the barge_in frame is not currently emitted when greeting is active and FSM state is IDLE.

- [ ] **Step 3: Add the flush guard**

In `server.py` `ws_handler`, find the `pcm_complete is not None` branch around line 748. The current code is:

```python
                pcm_complete = sess.feed_pcm(samples)
                if pcm_complete is None:
                    continue
                # Barge-in: if a turn is in progress, fire handle_barge_in
                # before starting a new turn.
                if pipeline.fsm.state in (State.THINKING, State.SPEAKING):
```

Insert the new guard **between** the `if pcm_complete is None: continue` and the existing `if pipeline.fsm.state in ...`:

```python
                pcm_complete = sess.feed_pcm(samples)
                if pcm_complete is None:
                    continue
                # Greeting flush: if greeting was just pushed and user
                # speech is now detected, tell the browser to flush its
                # playback queue IMMEDIATELY (before the new turn starts).
                # Without this, the greeting keeps playing for 200-500ms
                # until the new turn's first TTS PCM arrives. The new turn
                # then starts from a clean state.
                if sess.greeting_active:
                    sess.greeting_active = False
                    sess.tts_active = False
                    await sess.send_json({"type": "barge_in"})
                # Barge-in: if a turn is in progress, fire handle_barge_in
                # before starting a new turn.
                if pipeline.fsm.state in (State.THINKING, State.SPEAKING):
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_ws_integration.py::test_speech_during_greeting_emits_barge_in_immediately -v`
Expected: PASS

- [ ] **Step 5: Run full integration suite for regression**

Run: `python -m pytest tests/ -v`
Expected: all PASS

- [ ] **Step 6: Commit**

```bash
git add extensions/voice-assistant/service/server.py \
        extensions/voice-assistant/service/tests/test_ws_integration.py
git commit -m "feat(voice): flush greeting on first speech detection

Adds guard at top of pcm_complete branch: if greeting_active, emit
{type:barge_in} synchronously before starting new turn. Cuts greeting
interruption latency from 'new turn TTFB' to 'VAD silence timeout'."
```

---

## Task 8: Isolate greeting PCM in `measure_common`

**Files:**
- Modify: `extensions/voice-assistant/service/measure_common.py:60-137` (`run_one_turn`)
- Test: `extensions/voice-assistant/service/tests/test_greeting.py` (append) or `tests/test_measure_common.py` (new)

- [ ] **Step 1: Write the failing test**

Append to `tests/test_greeting.py`:

```python
def test_measure_common_counts_greeting_pcm_separately():
    """run_one_turn attributes PCM between greeting and asr_start to
    greeting_pcm_chunks, NOT tts_chunk_count. Verifies the measurement
    isolation that keeps Phase 2 metrics clean."""
    import asyncio
    import json
    import measure_common as mc
    from unittest.mock import AsyncMock, MagicMock

    # Fake WS that emits greeting + binary + asr_start + binary + stop
    class FakeWS:
        def __init__(self):
            self._sent = []
            self._idx = 0
            self.frames = [
                json.dumps({"type": "greeting", "text": "hi"}),
                b"\x01\x02" * 10,                            # greeting PCM
                json.dumps({"type": "asr_start"}),
                json.dumps({"type": "transcript", "text": "user"}),
                json.dumps({"type": "tts_start"}),
                b"\x03\x04" * 10,                            # turn PCM
                json.dumps({"type": "stop"}),
            ]
        async def send(self, x): self._sent.append(x)
        async def close(self): pass
        def __aiter__(self): return self
        async def __anext__(self):
            if self._idx >= len(self.frames): raise StopAsyncIteration
            f = self.frames[self._idx]; self._idx += 1
            return f

    # Mock websockets.connect to return an async-cm yielding FakeWS
    class FakeCM:
        def __init__(self, ws): self.ws = ws
        async def __aenter__(self): return self.ws
        async def __aexit__(self, *a): pass

    mc.websockets = MagicMock()
    mc.websockets.connect = lambda *a, **kw: FakeCM(FakeWS())

    # Stub the feed_audio inner task by making sleep a no-op
    mc.asyncio.sleep = AsyncMock()

    result = asyncio.run(mc.run_one_turn("ws://x", b"\x00" * 100))

    assert result["greeting_pcm_chunks"] == 1
    assert result["tts_chunk_count"] == 1  # only the post-tts_start binary
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_greeting.py::test_measure_common_counts_greeting_pcm_separately -v`
Expected: FAIL with `KeyError: 'greeting_pcm_chunks'` (field doesn't exist yet)

- [ ] **Step 3: Modify `run_one_turn` in `measure_common.py`**

In `measure_common.py` `run_one_turn`:

1. Add `greeting_pcm_chunks = 0` next to the other counters near the top
2. Add `greeting_seen = False` flag (flips True when the `greeting` text frame arrives; gates attribution of subsequent binary frames until `asr_start` / `tts_start` fire)
3. In the binary-frame branch, when `t_tts_start is None and not t_asr_done and greeting_seen`, increment `greeting_pcm_chunks` instead of `tts_chunk_count`
4. In the text-message branch, recognize `greeting` type — set `greeting_seen = True` and call `on_event(msg)` but DO NOT increment `llm_sentence_count`
5. Add `"greeting_pcm_chunks": greeting_pcm_chunks` to the returned dict

Concretely, modify the binary branch from:
```python
                if isinstance(raw, bytes):
                    tts_chunk_count += 1
                    now = time.perf_counter()
                    t_last_pcm = now
                    if t_tts_start is not None:
                        post_tts_pcm_chunks += 1
                        if t_first_tts_pcm is None:
                            t_first_tts_pcm = now
                    if on_event is not None:
                        on_event({"_binary": True, "t": now})
                    continue
```

To:
```python
                if isinstance(raw, bytes):
                    now = time.perf_counter()
                    t_last_pcm = now
                    # Greeting PCM arrives before any asr_start / tts_start.
                    # Count it separately so it doesn't pollute turn metrics.
                    if (t_asr_done is None and t_tts_start is None
                            and greeting_seen):
                        greeting_pcm_chunks += 1
                    else:
                        tts_chunk_count += 1
                        if t_tts_start is not None:
                            post_tts_pcm_chunks += 1
                            if t_first_tts_pcm is None:
                                t_first_tts_pcm = now
                    if on_event is not None:
                        on_event({"_binary": True, "t": now})
                    continue
```

And add `greeting` recognition in the text branch near `llm_sentence`:

```python
                if mtype == "transcript":
                    t_asr_done = time.perf_counter()
                elif mtype == "greeting":
                    greeting_seen = True
                elif mtype == "tts_start":
                    t_tts_start = time.perf_counter()
                elif mtype == "llm_sentence":
                    llm_sentence_count += 1
```

Initialize near the top of the function:
```python
    greeting_seen = False
    greeting_pcm_chunks = 0
```

And add to the returned dict:
```python
        "greeting_pcm_chunks": greeting_pcm_chunks,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_greeting.py::test_measure_common_counts_greeting_pcm_separately -v`
Expected: PASS

- [ ] **Step 5: Run bi-stream E2E dry-run (script must still import)**

Run: `python -c "import measure_bi_stream_e2e; import measure_neomind_e2e; print('imports OK')"`
Expected: prints `imports OK`

- [ ] **Step 6: Commit**

```bash
git add extensions/voice-assistant/service/measure_common.py \
        extensions/voice-assistant/service/tests/test_greeting.py
git commit -m "feat(voice): isolate greeting PCM in measure_common

Adds greeting_pcm_chunks counter; greeting binary frames no longer
pollute tts_chunk_count or post_tts_pcm_chunks. Phase 2 baseline
metrics stay clean."
```

---

## Task 9: Add `case 'greeting':` to poc.html

**Files:**
- Modify: `extensions/voice-assistant/service/poc.html` (around line 220-250, the message switch)
- Test: manual browser test only (no automated tests for HTML)

- [ ] **Step 1: Add the greeting case**

In `poc.html`, find the message switch (after `case 'transcript':` around line 220, before `case 'tts_start':` or wherever fits cleanly). Add:

```javascript
    case 'greeting':
      log(`greeting: "${obj.text}"`, 'warn');
      addTranscript(obj.text, 'auto');
      break;
```

(The PCM that follows goes through the existing playback queue — no JS change needed for audio.)

- [ ] **Step 2: Manual smoke test**

This step requires a running server + browser. The implementer can skip if no browser available; log it as "manual verification deferred":

```
1. Set interaction.greeting_text: "你好,我是 NeoMind 助手" in a test profile
2. Start server with that profile
3. Open poc.html in Chrome, connect
4. Verify: subtitle "你好,我是 NeoMind 助手" appears + audio plays immediately
5. Verify: speak during greeting → greeting cuts off, reply takes over
```

- [ ] **Step 3: Commit**

```bash
git add extensions/voice-assistant/service/poc.html
git commit -m "feat(voice): render greeting subtitle in poc.html

Adds case 'greeting' that displays greeting text in the transcript
area. PCM playback reuses the existing queue."
```

---

## Task 10: Document greeting in README

**Files:**
- Modify: `extensions/voice-assistant/README.md` (append new section after "NeoMind Integration")

- [ ] **Step 1: Append "Greeting (Say-First)" section**

In `extensions/voice-assistant/README.md`, append after the "NeoMind Integration" section:

```markdown

---

## Greeting (Say-First)

When a user connects, the assistant can immediately play a pre-synthesized
greeting clip ("你好,我是 NeoMind 助手") instead of waiting for the first
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
```

- [ ] **Step 2: Commit**

```bash
git add extensions/voice-assistant/README.md
git commit -m "docs(voice): document greeting (say-first) feature

Configuration, protocol, barge-in behavior, AEC opt-in rationale."
```

---

## Verification (after all tasks)

- [ ] **Step 1: Run full test suite**

```bash
cd extensions/voice-assistant/service
python -m pytest tests/ -v
```

Expected: all tests PASS, with at least 10 new tests added (3 profile + 3 warm_greeting + 1 encode_greeting + 3 ws_integration + 1 measure_common).

- [ ] **Step 2: Verify build.sh still works**

```bash
cd /Users/shenmingming/CamThink\ Project/NeoMind-Extensions
./build.sh --single voice-assistant --skip-frontend
```

Expected: builds successfully, produces `dist/voice-assistant-2.7.6-darwin_aarch64.nep`.

- [ ] **Step 3: Manual end-to-end smoke (optional, if env available)**

```bash
# Edit profiles/default.yaml to set greeting_text: "你好,我是 NeoMind 助手"
cd extensions/voice-assistant/service
VOICE_ASSISTANT_PROFILE=default python server.py --port 9384 &
# Open poc.html, connect, verify greeting plays immediately + subtitle shows
# Speak during greeting, verify barge-in cuts it off
```

Reset `greeting_text: ""` before committing if you changed it.
