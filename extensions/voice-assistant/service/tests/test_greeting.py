"""Unit tests for the greeting (say-first) feature."""
from __future__ import annotations

import asyncio
from unittest.mock import AsyncMock, MagicMock

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

    # Save originals so we restore them — patching mc.asyncio leaks globally
    # because asyncio is a shared stdlib module imported by reference.
    orig_websockets = mc.websockets
    orig_sleep = mc.asyncio.sleep
    try:
        mc.websockets = MagicMock()
        mc.websockets.connect = lambda *a, **kw: FakeCM(FakeWS())
        # Stub the feed_audio inner task by making sleep a no-op
        mc.asyncio.sleep = AsyncMock()

        result = asyncio.run(mc.run_one_turn("ws://x", b"\x00" * 100))
    finally:
        mc.websockets = orig_websockets
        mc.asyncio.sleep = orig_sleep

    assert result["greeting_pcm_chunks"] == 1
    assert result["tts_chunk_count"] == 1  # only the post-tts_start binary
