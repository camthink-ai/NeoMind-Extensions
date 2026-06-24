"""Smoke tests for the Silero VAD backend (B4).

These tests require:
  1. sherpa-onnx installed:        pip install sherpa-onnx>=1.10
  2. Silero backend enabled:       VOICE_ASSISTANT_VAD_BACKEND=silero python -m pytest test_silero_vad.py -v

If either precondition is missing, all tests skip rather than fail.
"""
from __future__ import annotations

import os
from pathlib import Path

import numpy as np
import pytest


# Cross-extension reference: voice-edge-tts bundles a real Chinese speech
# sample that we can use to exercise Silero with a known-voiced signal.
# Note: this file is 24kHz float32 (format 3), not 16kHz int16 — we resample
# in the test below.
_REAL_SPEECH_WAV = (
    Path(__file__).resolve().parents[3]
    / "voice-edge-tts" / "service" / "assets" / "default_prompt.wav"
)

# Silero VAD operates at 16kHz (server.py SAMPLE_RATE). Must match.
_TARGET_RATE = 16000


@pytest.fixture
def session():
    """A VoiceSession with Silero VAD instantiated, or pytest.skip."""
    import server as srv

    if srv.VAD_BACKEND != "silero" or srv._SILERO_VAD_CONFIG is None:
        pytest.skip(
            "Silero VAD not loaded — set VOICE_ASSISTANT_VAD_BACKEND=silero "
            "before running this test"
        )
    sess = srv.VoiceSession(ws=None, session_id="silero-test")
    if sess._silero_vad is None:
        pytest.skip("per-session Silero detector failed to init")
    return sess


def _feed_chunked(sess, audio_int16: np.ndarray, chunk: int = 512,
                  trail_silence_ms: int = 0):
    """Feed audio through sess.feed_pcm in chunk-sized pieces, collecting any
    returned utterance bytes. Returns list of byte strings.

    If trail_silence_ms > 0, appends that many milliseconds of digital silence
    after the main audio — Silero only emits completed segments after detecting
    post-speech silence, so the caller must pad trailing silence for speech
    samples that don't already end with a quiet tail.
    """
    results: list[bytes] = []
    if trail_silence_ms > 0:
        silence = np.zeros(_TARGET_RATE * trail_silence_ms // 1000, dtype=np.int16)
        audio_int16 = np.concatenate([audio_int16, silence])
    for i in range(0, len(audio_int16), chunk):
        piece = audio_int16[i:i + chunk]
        if piece.size < chunk:
            # Silero requires a full window; pad final partial chunk with zeros.
            piece = np.pad(piece, (0, chunk - piece.size))
        out = sess.feed_pcm(piece)
        if out is not None:
            results.append(out)
    return results


def _load_wav_as_int16(path: Path, target_rate: int = _TARGET_RATE) -> np.ndarray:
    """Load a WAV file (PCM or float) and convert to int16 at target_rate.

    Handles both int16 PCM (format 1) and IEEE float (format 3) WAVs, and
    performs linear resampling if the source rate differs from target.

    Python's stdlib ``wave`` module can't read float WAVs (format 3), so we
    prefer scipy.io.wavfile and fall back to manual binary parsing.
    """
    try:
        from scipy.io import wavfile
        sr, data = wavfile.read(str(path))
        # scipy returns int16 or float32 arrays depending on format.
        if data.dtype == np.int16:
            audio = data.astype(np.float32) / 32768.0
        elif data.dtype == np.float32:
            audio = data.copy()
        elif data.dtype == np.float64:
            audio = data.astype(np.float32)
        else:
            audio = data.astype(np.float32) / np.max(np.abs(data))
    except ImportError:
        # Fallback: manual binary parse (handles PCM + float WAVs without scipy)
        import struct
        with open(str(path), "rb") as f:
            raw = f.read()
        assert raw[:4] == b"RIFF", "not a RIFF file"
        # Walk chunks to find fmt + data
        pos = 12
        sr = n_channels = bits = audio_format = 0
        samples_bytes = b""
        while pos + 8 <= len(raw):
            chunk_id = raw[pos:pos + 4]
            chunk_len = struct.unpack_from("<I", raw, pos + 4)[0]
            body = raw[pos + 8:pos + 8 + chunk_len]
            if chunk_id == b"fmt ":
                audio_format, n_channels, sr = struct.unpack_from("<HHI", body, 0)
                bits = struct.unpack_from("<H", body, 14)[0]
            elif chunk_id == b"data":
                samples_bytes = body
            pos += 8 + chunk_len + (chunk_len & 1)
        if audio_format == 3 and bits == 32:
            audio = np.frombuffer(samples_bytes, dtype="<f4").copy()
        elif audio_format == 1 and bits == 16:
            audio = np.frombuffer(samples_bytes, dtype="<i2").astype(np.float32) / 32768.0
        else:
            raise ValueError(f"unsupported WAV: format={audio_format} bits={bits}")

    # Mix down to mono
    if audio.ndim > 1:
        audio = audio.mean(axis=1)

    # Linear resample if needed
    if sr != target_rate:
        n_out = int(round(len(audio) * target_rate / sr))
        indices = np.linspace(0, len(audio) - 1, n_out)
        audio = np.interp(indices, np.arange(len(audio)), audio)

    # float32 [-1, 1] → int16
    return (np.clip(audio, -1.0, 1.0) * 32767.0).astype("<i2")


def test_silence_does_not_fire(session):
    """Pure digital silence must never trigger Silero speech detection."""
    sil = np.zeros(16000, dtype=np.int16)  # 1 second
    outputs = _feed_chunked(session, sil)
    assert outputs == [], (
        f"Silero fired on pure silence ({len(outputs)} segments) — "
        "threshold may be too low"
    )


def test_real_speech_is_detected(session):
    """A real recorded speech sample (Chinese female, ~3s) must trigger at
    least one speech segment. Skips if the voice-edge-tts asset isn't
    available on disk (e.g., voice-edge-tts extension not yet built)."""
    if not _REAL_SPEECH_WAV.is_file():
        pytest.skip(
            f"real speech fixture missing: {_REAL_SPEECH_WAV} — "
            "build voice-edge-tts first or run its asset-bundling step"
        )

    audio = _load_wav_as_int16(_REAL_SPEECH_WAV)
    assert audio.size > 0, "fixture wav is empty after conversion"

    # Append 800ms of trailing silence so Silero detects speech-end and emits
    # the completed segment (SILERO_VAD_SILENCE_MS defaults to 500ms).
    outputs = _feed_chunked(session, audio, trail_silence_ms=800)
    assert outputs, (
        "Silero failed to detect speech in a real recorded sample — "
        "detector is misconfigured or model is broken"
    )
    # Sanity: at least one returned segment should be non-trivially long.
    longest = max(len(b) for b in outputs)
    assert longest >= 16000, (  # >= 0.5s of int16 = 16000 bytes
        f"detected segment too short ({longest} bytes) — likely a click/pop, "
        "not real speech"
    )
