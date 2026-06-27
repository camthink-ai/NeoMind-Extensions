"""End-to-end WebSocket integration test.

Drives the full /ws protocol path through FastAPI's TestClient:
  browser PCM (energy VAD) → mocked ASR → FakeLLMClient → mocked TTS → WS frames out

Validates the WS protocol contract that the browser-side Rust extension depends on.
No external services or model files required — energy VAD needs only numpy.
"""
from __future__ import annotations

import json
import time
from unittest.mock import AsyncMock, MagicMock

import numpy as np
import pytest
from fastapi.testclient import TestClient


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

@pytest.fixture
def ws_app(monkeypatch):
    """Configure server with energy VAD + mocked ASR/TTS + FakeLLM, return app.

    Force VAD_BACKEND=energy so VoiceSession skips sherpa-onnx Silero init
    (which needs the silero_vad.onnx model file). Lower VAD thresholds so the
    test PCM triggers detection in <500ms instead of the default 800ms.
    """
    import server

    monkeypatch.setattr(server, "VAD_BACKEND", "energy")
    monkeypatch.setattr(server, "VAD_ENERGY_THRESHOLD", 0.001)
    monkeypatch.setattr(server, "VAD_MIN_SPEECH_MS", 90)    # 3 frames @ 30ms
    monkeypatch.setattr(server, "VAD_SILENCE_MS", 150)      # 5 frames @ 30ms

    # Mock ASR — always returns a fixed transcript.
    mock_asr = MagicMock()
    mock_asr.transcribe = AsyncMock(return_value="你好")
    monkeypatch.setattr(server, "_asr_backend", mock_asr)

    # Mock TTS — returns short non-zero PCM so _tts_to_browser_pcm yields bytes.
    # Pipeline uses tts.stream() (async iterator of TtsChunk); also provide
    # synthesize() for ack-bank warmup compatibility.
    from contracts import TtsChunk
    mock_tts = MagicMock()
    mock_tts.synthesize = AsyncMock(return_value=b"\x10\x00" * 200)  # 200 int16 samples

    async def _tts_stream(text, voice):
        yield TtsChunk(pcm_int16=b"\x10\x00" * 200, sample_rate=24000, is_final=False)

    mock_tts.stream = _tts_stream
    monkeypatch.setattr(server, "_tts_backend", mock_tts)

    # Disable barge_in_ack so play_ack doesn't fire (no ack bank in test).
    monkeypatch.setattr(server._profile, "barge_in_ack", False)
    # Disable stage fillers so on_thinking_start doesn't try real TTS warmup.
    monkeypatch.setattr(server._profile, "stage_filler_words", {})

    # FakeLLMClient — implements LLMClient Protocol, yields Content then end.
    from backends.llm import FakeLLMClient
    fake_llm = FakeLLMClient(reply_template="你好啊,我是测试回复")
    monkeypatch.setattr(server, "make_llm", lambda profile: fake_llm)

    return server.app


# ---------------------------------------------------------------------------
# PCM generators
# ---------------------------------------------------------------------------

def _pcm_loud(duration_ms: int = 200, sr: int = 16000) -> bytes:
    """440Hz sine @ 0.5 amplitude → RMS ≈ 0.35, well above 0.001 threshold."""
    n = int(sr * duration_ms / 1000)
    t = np.arange(n) / sr
    samples = (0.5 * np.sin(2 * np.pi * 440 * t) * 32767).astype("<i2")
    return samples.tobytes()


def _pcm_silence(duration_ms: int = 200, sr: int = 16000) -> bytes:
    n = int(sr * duration_ms / 1000)
    return np.zeros(n, dtype="<i2").tobytes()


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _drain_ws(ws, expected_types: set[str], timeout_s: float = 3.0) -> tuple[list, list]:
    """Receive frames from WS until all expected_types seen or timeout.

    Returns (text_frames, binary_frames). Uses Starlette TestClient's
    polymorphic receive() which yields dicts with 'text' or 'bytes' keys.
    """
    deadline = time.monotonic() + timeout_s
    text_frames: list[dict] = []
    binary_frames: list[bytes] = []
    seen_types: set[str] = set()

    while time.monotonic() < deadline and not expected_types.issubset(seen_types):
        # Starlette TestClient: receive() blocks until any frame arrives.
        # No built-in timeout — we rely on the server emitting frames promptly.
        try:
            msg = ws.receive()
        except Exception:
            break
        if msg.get("type") == "websocket.disconnect":
            break
        if "bytes" in msg and msg["bytes"] is not None:
            binary_frames.append(msg["bytes"])
            continue
        text = msg.get("text")
        if text is None:
            continue
        try:
            obj = json.loads(text)
            text_frames.append(obj)
            seen_types.add(obj.get("type", ""))
        except json.JSONDecodeError:
            text_frames.append({"type": "_raw", "raw": text})

    return text_frames, binary_frames


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_ws_full_turn_emits_protocol_frames(ws_app):
    """A complete voice turn emits asr_start → transcript → tts_start →
    tts_end → stop in order, plus binary PCM."""
    client = TestClient(ws_app)
    expected = {"asr_start", "transcript", "tts_start", "tts_end", "stop"}

    with client.websocket_connect("/ws?session_id=int1") as ws:
        # 1. Send "start" handshake frame (server replies "ready")
        ws.send_text(json.dumps({"type": "start"}))

        # 2. Feed loud audio (triggers speech start after ~90ms)
        ws.send_bytes(_pcm_loud(200))
        # 3. Feed silence (triggers speech end after ~150ms)
        ws.send_bytes(_pcm_silence(200))

        # 4. Drain frames
        text_frames, _ = _drain_ws(ws, expected, timeout_s=3.0)

    types = [f.get("type") for f in text_frames if isinstance(f, dict)]

    # ready is the reply to start
    assert "ready" in types, f"missing ready; got {types}"
    # All expected turn lifecycle frames must be present
    for t in expected:
        assert t in types, f"missing frame type {t}; got {types}"

    # Verify relative order (asr_start before transcript before tts_start before stop)
    def idx(t):
        return types.index(t)
    assert idx("asr_start") < idx("transcript"), f"order wrong: {types}"
    assert idx("transcript") < idx("tts_start"), f"order wrong: {types}"
    assert idx("tts_start") < idx("tts_end"), f"order wrong: {types}"
    assert idx("tts_end") < idx("stop"), f"order wrong: {types}"

    # transcript frame carries the mocked ASR text
    transcript_frame = next(f for f in text_frames if f.get("type") == "transcript")
    assert transcript_frame["text"] == "你好"
    assert "elapsed_ms" in transcript_frame

    # tts_start carries the mode marker
    tts_start_frame = next(f for f in text_frames if f.get("type") == "tts_start")
    assert tts_start_frame["mode"] == "full_synthesize"


def test_ws_empty_transcript_emits_skip(ws_app, monkeypatch):
    """When ASR returns empty/whitespace, server emits skip frame (no tts_start)."""
    # Override ASR mock to return whitespace
    import server
    mock_asr_empty = MagicMock()
    mock_asr_empty.transcribe = AsyncMock(return_value="   ")
    monkeypatch.setattr(server, "_asr_backend", mock_asr_empty)

    client = TestClient(ws_app)
    expected = {"asr_start", "skip", "stop"}

    with client.websocket_connect("/ws?session_id=int2") as ws:
        ws.send_bytes(_pcm_loud(200))
        ws.send_bytes(_pcm_silence(200))
        text_frames, _ = _drain_ws(ws, expected, timeout_s=3.0)

    types = [f.get("type") for f in text_frames if isinstance(f, dict)]
    assert "skip" in types, f"missing skip; got {types}"
    assert "tts_start" not in types, "TTS should not start on empty transcript"
    assert "stop" in types, f"missing stop; got {types}"


def test_ws_ping_pong(ws_app):
    """Ping frame gets an immediate pong reply."""
    client = TestClient(ws_app)
    with client.websocket_connect("/ws?session_id=int3") as ws:
        ws.send_text(json.dumps({"type": "ping"}))
        msg = ws.receive_text()
        obj = json.loads(msg)
        assert obj["type"] == "pong"


def test_ws_client_stop_triggers_no_phantom_pipeline(ws_app):
    """A client `stop` frame without any prior audio doesn't crash the server."""
    client = TestClient(ws_app)
    with client.websocket_connect("/ws?session_id=int4") as ws:
        ws.send_text(json.dumps({"type": "stop"}))
        # No assertions on response — just verify the connection stays open.
        # Send a ping to confirm the server is still responsive.
        ws.send_text(json.dumps({"type": "ping"}))
        msg = ws.receive_text()
        assert json.loads(msg)["type"] == "pong"


# ---------------------------------------------------------------------------
# Greeting (say-first) — Task 6
# ---------------------------------------------------------------------------

def test_start_emits_greeting_when_enabled(monkeypatch):
    """When _GREETING_PCM is populated, start frame triggers
    ready -> greeting -> binary PCM, in that order."""
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
    from contracts import TtsChunk
    mock_asr = MagicMock(); mock_asr.transcribe = AsyncMock(return_value="hi")
    monkeypatch.setattr(server, "_asr_backend", mock_asr)
    mock_tts = MagicMock()
    mock_tts.synthesize = AsyncMock(return_value=b"\x10\x00" * 100)
    async def _stream(text, voice):
        yield TtsChunk(pcm_int16=b"\x10\x00" * 100, sample_rate=24000, is_final=True)
    mock_tts.stream = _stream
    monkeypatch.setattr(server, "_tts_backend", mock_tts)

    client = TestClient(server.app)

    with client.websocket_connect("/ws?session_id=test-greet") as ws:
        ws.send_json({"type": "start", "sample_rate": 16000})
        text_frames, binary_frames = _drain_ws(
            ws, expected_types={"ready", "greeting"}, timeout_s=3.0)
        # The server's greeting push sends text frame THEN binary frame
        # synchronously. _drain_ws exits as soon as both text types are
        # seen, so the binary PCM is still pending in the WS buffer.
        # Do ONE more receive to grab it — a single frame is guaranteed to
        # be available because the server completed send_binary() before
        # yielding. Do NOT loop (ws.receive() has no timeout and would
        # block forever on an empty queue).
        extra = ws.receive()
        if "bytes" in extra and extra["bytes"] is not None:
            binary_frames.append(extra["bytes"])

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
