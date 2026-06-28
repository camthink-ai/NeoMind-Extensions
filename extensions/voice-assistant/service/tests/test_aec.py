"""Unit tests for ReferenceRingBuffer and AEC backends."""
import numpy as np
import pytest


# ---------------------------------------------------------------------------
# ReferenceRingBuffer
# ---------------------------------------------------------------------------

def test_ring_buffer_push_then_peek_full():
    """push(N bytes) then peek_window(0, full_duration_ms) returns those bytes."""
    from aec import ReferenceRingBuffer
    SAMPLE_RATE = 16000
    # 1 second capacity
    buf = ReferenceRingBuffer(capacity_bytes=SAMPLE_RATE * 2 * 1)
    # Push 100ms of distinctive samples
    samples = np.arange(1600, dtype="<i2")
    buf.push(samples.tobytes())
    out = buf.peek_window(delay_ms=0, length_ms=100, sample_rate=SAMPLE_RATE)
    out_samples = np.frombuffer(out, dtype="<i2")
    assert len(out_samples) == 1600
    assert (out_samples == samples).all()


def test_ring_buffer_delay_window():
    """peek_window(delay_ms=200, length_ms=100) returns the slice from 200ms ago."""
    from aec import ReferenceRingBuffer
    SAMPLE_RATE = 16000
    buf = ReferenceRingBuffer(capacity_bytes=SAMPLE_RATE * 2 * 2)  # 2s capacity
    # Push 300ms of zeros, then 100ms of ones (the "marker")
    buf.push(np.zeros(4800, dtype="<i2").tobytes())  # 300ms zeros
    marker = np.ones(1600, dtype="<i2")
    buf.push(marker.tobytes())  # 100ms ones at the end
    # Now: most-recent 100ms is the marker (delay=0); 200ms-100ms ago is also zeros
    # But we want 200ms ago for 100ms length — that's all zeros
    out = buf.peek_window(delay_ms=200, length_ms=100, sample_rate=SAMPLE_RATE)
    out_samples = np.frombuffer(out, dtype="<i2")
    assert len(out_samples) == 1600
    assert (out_samples == 0).all(), "200ms ago was still in the zeros region"


def test_ring_buffer_zero_pads_on_underflow():
    """When delay exceeds capacity, peek_window zero-pads missing prefix."""
    from aec import ReferenceRingBuffer
    SAMPLE_RATE = 16000
    # Tiny buffer: 100ms capacity
    buf = ReferenceRingBuffer(capacity_bytes=SAMPLE_RATE * 2 * 100 // 1000)
    buf.push(np.ones(1600, dtype="<i2").tobytes())  # 100ms of ones
    # Request 200ms window at delay=0; only 100ms is available, should zero-pad prefix
    out = buf.peek_window(delay_ms=0, length_ms=200, sample_rate=SAMPLE_RATE)
    out_samples = np.frombuffer(out, dtype="<i2")
    assert len(out_samples) == 3200
    # First 1600 should be zeros (padding), last 1600 should be ones
    assert (out_samples[:1600] == 0).all()
    assert (out_samples[1600:] == 1).all()


def test_ring_buffer_fifo_wrap():
    """Pushing beyond capacity drops oldest data (FIFO)."""
    from aec import ReferenceRingBuffer
    SAMPLE_RATE = 16000
    buf = ReferenceRingBuffer(capacity_bytes=SAMPLE_RATE * 2 * 200 // 1000)  # 200ms
    first = np.full(1600, 1, dtype="<i2")  # 100ms of 1s
    second = np.full(1600, 2, dtype="<i2")  # 100ms of 2s
    third = np.full(1600, 3, dtype="<i2")  # 100ms of 3s — should evict `first`
    buf.push(first.tobytes())
    buf.push(second.tobytes())
    buf.push(third.tobytes())
    out = buf.peek_window(delay_ms=0, length_ms=200, sample_rate=SAMPLE_RATE)
    out_samples = np.frombuffer(out, dtype="<i2")
    # `first` (1s) should be gone; we should see 2s then 3s
    assert (out_samples[:1600] == 2).all()
    assert (out_samples[1600:] == 3).all()


def test_ring_buffer_empty_peek_returns_zeros():
    """Empty buffer peek returns all zeros of the requested length."""
    from aec import ReferenceRingBuffer
    SAMPLE_RATE = 16000
    buf = ReferenceRingBuffer(capacity_bytes=SAMPLE_RATE * 2 * 1)
    out = buf.peek_window(delay_ms=0, length_ms=100, sample_rate=SAMPLE_RATE)
    out_samples = np.frombuffer(out, dtype="<i2")
    assert len(out_samples) == 1600
    assert (out_samples == 0).all()


def test_ring_buffer_push_truncates_odd_length_input():
    """Odd-length input is truncated by one byte to preserve int16 alignment."""
    from aec import ReferenceRingBuffer
    SAMPLE_RATE = 16000
    buf = ReferenceRingBuffer(capacity_bytes=SAMPLE_RATE * 2 * 1)
    # 1601 bytes (800 samples + 1 trailing byte)
    odd_input = np.arange(800, dtype="<i2").tobytes() + b"\x7f"
    buf.push(odd_input)
    out = buf.peek_window(delay_ms=0, length_ms=50, sample_rate=SAMPLE_RATE)
    out_samples = np.frombuffer(out, dtype="<i2")
    # Should be the first 800 samples (last 800 of arange(800) since capacity>input),
    # no garbage from the odd trailing byte
    assert len(out_samples) == 800
    assert (out_samples == np.arange(800, dtype="<i2")).all()


# ---------------------------------------------------------------------------
# NoopAECBackend
# ---------------------------------------------------------------------------

def test_noop_aec_returns_input_unchanged():
    """NoopAECBackend.process_capture returns mic PCM unchanged."""
    from backends.aec import NoopAECBackend
    backend = NoopAECBackend()
    assert backend.init(16000) is True
    mic = np.arange(1600, dtype="<i2")
    ref = np.zeros(1600, dtype="<i2")
    out = backend.process_capture(mic, ref)
    assert (out == mic).all()


def test_noop_aec_close_is_noop():
    from backends.aec import NoopAECBackend
    backend = NoopAECBackend()
    backend.close()  # must not raise


# ---------------------------------------------------------------------------
# WebRtcAECBackend (webrtc_audio_processing mocked)
# ---------------------------------------------------------------------------

def test_webrtc_aec_init_success(monkeypatch):
    """When webrtc_audio_processing is importable, init() returns True and configures formats."""
    from backends import aec as aec_module
    from backends.aec import WebRtcAECBackend

    recorded = {}

    class FakeAPM:
        def __init__(self, **kwargs):
            recorded["init_kwargs"] = kwargs
            self.calls = []

        def set_stream_format(self, *a):
            self.calls.append(("set_stream_format", a))

        def set_reverse_stream_format(self, *a):
            self.calls.append(("set_reverse_stream_format", a))

        def set_aec_level(self, *a):
            self.calls.append(("set_aec_level", a))

        def set_system_delay(self, *a):
            self.calls.append(("set_system_delay", a))

        def process_reverse_stream(self, *a):
            self.calls.append(("process_reverse_stream", a))

        def process_stream(self, *a):
            self.calls.append(("process_stream", a))
            return a[0]  # passthrough

    monkeypatch.setattr(aec_module, "_WEBRTC_APM_CLASS", FakeAPM)
    backend = WebRtcAECBackend()
    assert backend.init(16000) is True
    assert backend._apm is not None
    # Verify format configuration was set for both directions
    method_names = [c[0] for c in backend._apm.calls]
    assert "set_stream_format" in method_names
    assert "set_reverse_stream_format" in method_names
    assert "set_aec_level" in method_names
    assert recorded["init_kwargs"] == {"aec_type": 2}


def test_webrtc_aec_init_failure_returns_false(monkeypatch):
    """If AudioProcessingModule constructor raises, init returns False."""
    from backends import aec as aec_module
    from backends.aec import WebRtcAECBackend

    class FakeAPM:
        def __init__(self, **kwargs):
            raise RuntimeError("native lib load failed")

    monkeypatch.setattr(aec_module, "_WEBRTC_APM_CLASS", FakeAPM)
    backend = WebRtcAECBackend()
    assert backend.init(16000) is False


def test_webrtc_aec_init_returns_false_when_class_unavailable(monkeypatch):
    """When _WEBRTC_APM_CLASS resolves to None (library not installed), init returns False."""
    from backends import aec as aec_module
    from backends.aec import WebRtcAECBackend

    monkeypatch.setattr(aec_module, "_WEBRTC_APM_CLASS", None)
    backend = WebRtcAECBackend()
    assert backend.init(16000) is False


def test_webrtc_aec_process_capture_alternates_ref_and_mic(monkeypatch):
    """For each 10ms frame, process_capture feeds reference then capture."""
    from backends import aec as aec_module
    from backends.aec import WebRtcAECBackend

    class FakeAPM:
        def __init__(self, **kwargs):
            self.call_sequence = []

        def set_stream_format(self, *a):
            pass

        def set_reverse_stream_format(self, *a):
            pass

        def set_aec_level(self, *a):
            pass

        def set_system_delay(self, *a):
            self.call_sequence.append("set_system_delay")

        def process_reverse_stream(self, data):
            self.call_sequence.append(("reverse", len(data)))

        def process_stream(self, data):
            self.call_sequence.append(("capture", len(data)))
            return data

    monkeypatch.setattr(aec_module, "_WEBRTC_APM_CLASS", FakeAPM)
    backend = WebRtcAECBackend()
    assert backend.init(16000)
    # Two 10ms frames: mic is 320 samples (20ms), ref same length
    mic = np.arange(320, dtype="<i2")
    ref = np.zeros(320, dtype="<i2")
    out = backend.process_capture(mic, ref)
    assert isinstance(out, np.ndarray)
    assert out.dtype == "<i2"
    assert len(out) == 320
    # Verify per-frame ordering: reverse then capture, twice
    interesting = [c for c in backend._apm.call_sequence if isinstance(c, tuple)]
    assert len(interesting) == 4
    assert interesting[0][0] == "reverse"
    assert interesting[1][0] == "capture"
    assert interesting[2][0] == "reverse"
    assert interesting[3][0] == "capture"


def test_webrtc_aec_process_capture_short_frame_padding(monkeypatch):
    """Non-multiple-of-10ms inputs are zero-padded and output is truncated back."""
    from backends import aec as aec_module
    from backends.aec import WebRtcAECBackend

    class FakeAPM:
        def __init__(self, **kwargs):
            self.processed_frame_lengths = []

        def set_stream_format(self, *a):
            pass

        def set_reverse_stream_format(self, *a):
            pass

        def set_aec_level(self, *a):
            pass

        def set_system_delay(self, *a):
            pass

        def process_reverse_stream(self, data):
            self.processed_frame_lengths.append(("reverse", len(data)))

        def process_stream(self, data):
            self.processed_frame_lengths.append(("capture", len(data)))
            return data

    monkeypatch.setattr(aec_module, "_WEBRTC_APM_CLASS", FakeAPM)
    backend = WebRtcAECBackend()
    assert backend.init(16000)
    # 240 samples = 1.5 frames; should be padded to 2 full 320-byte frames internally
    mic = np.arange(240, dtype="<i2")
    ref = np.zeros(240, dtype="<i2")
    out = backend.process_capture(mic, ref)
    assert isinstance(out, np.ndarray)
    assert out.dtype == "<i2"
    assert len(out) == 240, "output truncated back to original mic length"
    # Both internal frames should be exactly 320 bytes
    captures = [n for kind, n in backend._apm.processed_frame_lengths if kind == "capture"]
    assert captures == [320, 320], "padding added to reach 10ms boundary"


def test_webrtc_aec_passthrough_when_uninit(monkeypatch):
    """If init() never succeeded, process_capture returns mic unchanged."""
    from backends import aec as aec_module
    from backends.aec import WebRtcAECBackend

    monkeypatch.setattr(aec_module, "_WEBRTC_APM_CLASS", None)
    backend = WebRtcAECBackend()
    # init() returns False because class is None
    assert backend.init(16000) is False
    mic = np.arange(160, dtype="<i2")
    ref = np.zeros(160, dtype="<i2")
    out = backend.process_capture(mic, ref)
    assert (out == mic).all()


# ---------------------------------------------------------------------------
# make_aec factory
# ---------------------------------------------------------------------------

class _FakeProfile:
    """Minimal Profile stand-in for factory tests."""
    def __init__(self, aec_config):
        self.aec_config = aec_config


def test_make_aec_returns_noop_for_none_config():
    """profile.aec_config=None -> NoopAECBackend."""
    from backends import make_aec
    from backends.aec import NoopAECBackend
    backend = make_aec(_FakeProfile(aec_config=None))
    assert isinstance(backend, NoopAECBackend)


def test_make_aec_returns_noop_for_echo_window_config():
    """profile.aec_config={'type':'echo_window'} -> NoopAECBackend
    (echo_window runs in VAD, not the AEC backend)."""
    from backends import make_aec
    from backends.aec import NoopAECBackend
    backend = make_aec(_FakeProfile(aec_config={
        "type": "echo_window", "reference_delay_ms": 200,
        "ref_buffer_seconds": 3.0, "keep_echo_window": True,
    }))
    assert isinstance(backend, NoopAECBackend)


def test_make_aec_returns_webrtc_when_available(monkeypatch):
    """profile.aec_config={'type':'webrtc'} -> WebRtcAECBackend when import works."""
    from backends import make_aec
    from backends import aec as aec_module
    from backends.aec import WebRtcAECBackend

    class FakeAPM:
        def __init__(self, **kwargs):
            pass
        def set_stream_format(self, *a): pass
        def set_reverse_stream_format(self, *a): pass
        def set_aec_level(self, *a): pass

    monkeypatch.setattr(aec_module, "_WEBRTC_APM_CLASS", FakeAPM)

    backend = make_aec(_FakeProfile(aec_config={
        "type": "webrtc", "reference_delay_ms": 200,
        "ref_buffer_seconds": 3.0, "keep_echo_window": False,
    }))
    assert isinstance(backend, WebRtcAECBackend)


def test_make_aec_falls_back_to_noop_on_import_failure(monkeypatch, caplog):
    """profile.aec_config={'type':'webrtc'} + webrtc unavailable -> Noop + warning."""
    import logging
    from backends import make_aec
    from backends import aec as aec_module
    from backends.aec import NoopAECBackend

    # Force _resolve_webrtc_apm_class to return None
    monkeypatch.setattr(aec_module, "_WEBRTC_APM_CLASS", None)
    monkeypatch.setattr(aec_module, "_resolve_webrtc_apm_class", lambda: None)

    with caplog.at_level(logging.WARNING):
        backend = make_aec(_FakeProfile(aec_config={
            "type": "webrtc", "reference_delay_ms": 200,
            "ref_buffer_seconds": 3.0, "keep_echo_window": False,
        }))
    assert isinstance(backend, NoopAECBackend)
    assert any("webrtc" in r.message.lower() and "fallback" in r.message.lower()
               for r in caplog.records), \
        "Expected a warning mentioning webrtc and fallback"


def test_make_aec_unknown_type_raises():
    """Unknown type raises ValueError (programmer error, not runtime fallback)."""
    from backends import make_aec
    with pytest.raises(ValueError, match="unknown AEC backend"):
        make_aec(_FakeProfile(aec_config={"type": "bogus"}))


# ---------------------------------------------------------------------------
# Server startup reconciliation (Task 7)
# ---------------------------------------------------------------------------

def test_startup_reconciles_aec_mode_from_profile(monkeypatch):
    """_warm_banks_async sets AEC_MODE from profile.aec_config['type']."""
    import asyncio
    import server
    from backends import aec as aec_module

    # Force webrtc "available"
    class FakeAPM:
        def __init__(self, **kwargs):
            pass
        def set_stream_format(self, *a): pass
        def set_reverse_stream_format(self, *a): pass
        def set_aec_level(self, *a): pass
    monkeypatch.setattr(aec_module, "_WEBRTC_APM_CLASS", FakeAPM)

    # Profile says webrtc
    monkeypatch.setattr(server, "_profile", type("P", (), {
        "aec_config": {"type": "webrtc", "reference_delay_ms": 200,
                       "ref_buffer_seconds": 3.0, "keep_echo_window": False},
        "greeting_text": "",
        "barge_in_ack": False,
        "ack_words": [],
        "stage_filler_words": {},
    })())
    # No env override
    monkeypatch.delenv("VOICE_ASSISTANT_AEC_MODE", raising=False)
    # Reset module globals to defaults so the test is deterministic
    monkeypatch.setattr(server, "_aec_backend", None, raising=False)
    monkeypatch.setattr(server, "_ref_ring_buffer", None, raising=False)

    asyncio.run(server._warm_banks_async())

    assert server.AEC_MODE == "webrtc"
    from backends.aec import WebRtcAECBackend
    assert isinstance(server._aec_backend, WebRtcAECBackend)
    assert server._ref_ring_buffer is not None


def test_startup_env_override_wins(monkeypatch):
    """VOICE_ASSISTANT_AEC_MODE env var overrides profile for debugging."""
    import asyncio
    import server

    monkeypatch.setattr(server, "_profile", type("P", (), {
        "aec_config": {"type": "webrtc", "reference_delay_ms": 200,
                       "ref_buffer_seconds": 3.0, "keep_echo_window": False},
        "greeting_text": "",
        "barge_in_ack": False,
        "ack_words": [],
        "stage_filler_words": {},
    })())
    monkeypatch.setenv("VOICE_ASSISTANT_AEC_MODE", "echo_window")
    monkeypatch.setattr(server, "_aec_backend", None, raising=False)
    monkeypatch.setattr(server, "_ref_ring_buffer", None, raising=False)

    asyncio.run(server._warm_banks_async())

    assert server.AEC_MODE == "echo_window"
    from backends.aec import NoopAECBackend
    assert isinstance(server._aec_backend, NoopAECBackend)


def test_startup_falls_back_to_noop_when_backend_init_fails(monkeypatch):
    """When make_aec returns a backend whose init() raises, _warm_aec
    substitutes NoopAECBackend so the pipeline always has a usable backend."""
    import asyncio
    import server
    from backends import aec as aec_module

    # FakeAPM constructs OK but a downstream config call raises,
    # exercising the server-layer fallback (not just the factory one).
    class FakeAPM:
        def __init__(self, **kwargs):
            pass
        def set_stream_format(self, *a):
            raise RuntimeError("native config failure")
        def set_reverse_stream_format(self, *a): pass
        def set_aec_level(self, *a): pass
    monkeypatch.setattr(aec_module, "_WEBRTC_APM_CLASS", FakeAPM)

    monkeypatch.setattr(server, "_profile", type("P", (), {
        "aec_config": {"type": "webrtc", "reference_delay_ms": 200,
                       "ref_buffer_seconds": 3.0, "keep_echo_window": False},
        "greeting_text": "",
        "barge_in_ack": False,
        "ack_words": [],
        "stage_filler_words": {},
    })())
    monkeypatch.delenv("VOICE_ASSISTANT_AEC_MODE", raising=False)
    # Reset module globals so the idempotency short-circuit doesn't skip the test
    monkeypatch.setattr(server, "_aec_backend", None, raising=False)
    monkeypatch.setattr(server, "_ref_ring_buffer", None, raising=False)

    asyncio.run(server._warm_banks_async())

    assert server.AEC_MODE == "webrtc"
    from backends.aec import NoopAECBackend
    assert isinstance(server._aec_backend, NoopAECBackend), \
        "server must substitute Noop when backend init fails"
    assert server._ref_ring_buffer is not None


# ---------------------------------------------------------------------------
# feed_pcm AEC preprocessing hook (Task 8)
# ---------------------------------------------------------------------------

def test_feed_pcm_aec_spy(monkeypatch):
    """Spy on process_capture to verify feed_pcm invokes it exactly once."""
    import numpy as np
    import server

    calls = []
    class SpyBackend:
        def init(self, sr): return True
        def process_capture(self, mic, ref):
            calls.append((mic.copy(), ref.copy()))
            return mic
        def close(self): pass

    monkeypatch.setattr(server, "_aec_backend", SpyBackend())
    fake_buf = type("B", (), {"peek_window": lambda self, **kw: np.zeros(480, dtype="<i2").tobytes()})()
    monkeypatch.setattr(server, "_ref_ring_buffer", fake_buf)

    sess = server.VoiceSession.__new__(server.VoiceSession)
    sess._silero_vad = None
    sess._fsmn_vad = None
    monkeypatch.setattr(server, "VAD_BACKEND", "energy")
    monkeypatch.setattr(server, "VAD_ENERGY_THRESHOLD", 999.0)
    monkeypatch.setattr(server, "VAD_MIN_SPEECH_MS", 9999)
    monkeypatch.setattr(server, "VAD_SILENCE_MS", 9999)
    sess.in_speech = False
    sess.speech_frames = 0
    sess._speech_buffer = []
    # _feed_pcm_energy calls _aec_active_now which reads these; tts_active=False
    # short-circuits before tts_last_chunk_ts is touched.
    sess.tts_active = False
    sess.tts_last_chunk_ts = 0.0

    sess.feed_pcm(np.full(480, 50, dtype="<i2"))
    assert len(calls) == 1
    assert (calls[0][0] == 50).all()


def test_feed_pcm_skips_aec_when_backend_is_noop(monkeypatch):
    """When _aec_backend is NoopAECBackend, feed_pcm does NOT call process_capture."""
    import numpy as np
    import server
    from backends.aec import NoopAECBackend

    calls = []
    orig = NoopAECBackend.process_capture
    def spy(self, m, r):
        calls.append((m, r))
        return orig(self, m, r)
    monkeypatch.setattr(NoopAECBackend, "process_capture", spy)

    monkeypatch.setattr(server, "_aec_backend", NoopAECBackend())

    sess = server.VoiceSession.__new__(server.VoiceSession)
    sess._silero_vad = None
    sess._fsmn_vad = None
    monkeypatch.setattr(server, "VAD_BACKEND", "energy")
    monkeypatch.setattr(server, "VAD_ENERGY_THRESHOLD", 999.0)
    monkeypatch.setattr(server, "VAD_MIN_SPEECH_MS", 9999)
    monkeypatch.setattr(server, "VAD_SILENCE_MS", 9999)
    sess.in_speech = False
    sess.speech_frames = 0
    sess._speech_buffer = []
    # Required by _aec_active_now (called inside _feed_pcm_energy)
    sess.tts_active = False
    sess.tts_last_chunk_ts = 0.0

    sess.feed_pcm(np.full(480, 50, dtype="<i2"))
    assert len(calls) == 0, "Noop should be short-circuited (perf optimization)"


def test_feed_pcm_aec_exception_passthrough(monkeypatch):
    """If process_capture raises, feed_pcm logs and passes original mic to VAD."""
    import numpy as np
    import server
    from backends.aec import NoopAECBackend

    class ExplodingBackend:
        def init(self, sr): return True
        def process_capture(self, mic, ref):
            raise RuntimeError("native explosion")
        def close(self): pass

    monkeypatch.setattr(server, "_aec_backend", ExplodingBackend())
    fake_buf = type("B", (), {"peek_window": lambda self, **kw: np.zeros(480, dtype="<i2").tobytes()})()
    monkeypatch.setattr(server, "_ref_ring_buffer", fake_buf)

    sess = server.VoiceSession.__new__(server.VoiceSession)
    sess._silero_vad = None
    sess._fsmn_vad = None
    # Replace _feed_pcm_energy with a spy via bound method
    captured = []
    def energy_spy(self_, samples):
        captured.append(samples.copy())
        return None
    monkeypatch.setattr(server.VoiceSession, "_feed_pcm_energy", energy_spy)
    sess.in_speech = False
    sess.speech_frames = 0
    sess._speech_buffer = []

    mic = np.full(480, 50, dtype="<i2")
    sess.feed_pcm(mic)
    # VAD path received the ORIGINAL mic (not the exploded result)
    assert len(captured) == 1
    assert (captured[0] == 50).all()
    # Critical: the module global _aec_backend is NOT mutated (no cross-session downgrade)
    assert isinstance(server._aec_backend, ExplodingBackend), \
        "module global must NOT be downgraded on per-frame exception"


# ---------------------------------------------------------------------------
# send_binary ref buffer instrumentation (Task 9)
# ---------------------------------------------------------------------------

def test_send_binary_pushes_to_ref_buffer(monkeypatch):
    """VoiceSession.send_binary should also push to the global ref ring buffer."""
    import asyncio
    import server

    pushed = []
    fake_buf = type("B", (), {"push": lambda self, b: pushed.append(b)})()
    monkeypatch.setattr(server, "_ref_ring_buffer", fake_buf)

    # Mock the underlying ws
    class FakeWS:
        def __init__(self): self.sent = []
        async def send_bytes(self, b): self.sent.append(b)
    fake_ws = FakeWS()

    sess = server.VoiceSession.__new__(server.VoiceSession)
    sess.ws = fake_ws
    sess.bytes_out = 0

    payload = b"\x01\x02" * 100
    asyncio.run(sess.send_binary(payload))

    assert pushed == [payload], "send_binary must mirror to ref ring buffer"
    assert fake_ws.sent == [payload], "ws.send_bytes still called once"


# ---------------------------------------------------------------------------
# _aec_active_now tightening (Task 10)
# ---------------------------------------------------------------------------

def test_aec_active_now_only_for_echo_window_mode(monkeypatch):
    """When AEC_MODE is sherpa/webrtc/none, _aec_active_now returns False
    even if tts_active is True. Only echo_window uses the boost."""
    import time
    import server

    sess = server.VoiceSession.__new__(server.VoiceSession)
    sess.tts_active = True
    sess.tts_last_chunk_ts = time.perf_counter()  # immediate

    for mode in ("sherpa", "webrtc", "none"):
        monkeypatch.setattr(server, "AEC_MODE", mode)
        assert sess._aec_active_now() is False, f"mode={mode} must not activate echo_window"

    monkeypatch.setattr(server, "AEC_MODE", "echo_window")
    assert sess._aec_active_now() is True
