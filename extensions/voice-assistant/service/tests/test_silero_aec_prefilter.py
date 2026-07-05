"""Silero VAD echo-window energy pre-filter tests.

Validates that _feed_pcm_silero no longer mutes the mic entirely during
TTS playback in echo_window mode. Instead, an RMS energy pre-filter
skips the Silero feed for quiet (TTS echo) input but lets loud (user
speech) input through to Silero for barge-in.

This is the fix that makes barge-in actually work on macOS / hosts
where webrtc-audio-processing is unavailable and AEC degrades to
echo_window half-duplex.
"""
from __future__ import annotations

import time
from unittest.mock import MagicMock

import numpy as np
import pytest


def _make_session(monkeypatch, *, aec_active: bool):
    """Build a minimal VoiceSession with a fake _silero_vad.

    ``aec_active`` controls what _aec_active_now() will return: when True,
    tts_active is set and tts_last_chunk_ts is fresh so the AEC window
    is open.
    """
    import server

    # Force echo_window mode so _aec_active_now() can fire.
    monkeypatch.setattr(server, "AEC_MODE", "echo_window")
    # Reasonable energy threshold + boost so we can construct test PCM
    # above and below the cutoff.
    monkeypatch.setattr(server, "VAD_ENERGY_THRESHOLD", 0.010)
    monkeypatch.setattr(server, "AEC_ENERGY_BOOST", 0.020)
    monkeypatch.setattr(server, "AEC_TAIL_MS", 10_000)  # window never expires

    fake_vad = MagicMock()
    fake_vad.empty = MagicMock(return_value=True)  # no segments complete
    fake_vad.accept_waveform = MagicMock()

    sess = server.VoiceSession.__new__(server.VoiceSession)
    sess._silero_vad = fake_vad
    if aec_active:
        sess.tts_active = True
        sess.tts_last_chunk_ts = time.perf_counter()
    else:
        sess.tts_active = False
        sess.tts_last_chunk_ts = 0.0
    return sess, fake_vad


def test_silero_skips_feed_when_echo_window_and_quiet_input(monkeypatch):
    """During echo_window, mic input below energy boost is treated as TTS
    echo and never reaches Silero. No false barge-in from speaker echo."""
    sess, fake_vad = _make_session(monkeypatch, aec_active=True)

    # Quiet input: amplitude ~0.001 → RMS ~0.001, well below
    # (0.010 + 0.020) = 0.030 cutoff.
    quiet = (np.ones(480, dtype=np.float32) * 0.001 * 32768).astype("<i2")
    result = sess._feed_pcm_silero(quiet)

    assert result is None
    fake_vad.accept_waveform.assert_not_called()


def test_silero_feeds_when_echo_window_and_loud_input(monkeypatch):
    """During echo_window, loud mic input (user speaking over TTS) reaches
    Silero so barge-in can fire."""
    sess, fake_vad = _make_session(monkeypatch, aec_active=True)

    # Loud input: amplitude ~0.5 → RMS ~0.5, well above 0.030 cutoff.
    loud = (np.ones(480, dtype=np.float32) * 0.5 * 32768).astype("<i2")
    sess._feed_pcm_silero(loud)

    fake_vad.accept_waveform.assert_called_once()


def test_silero_feeds_normally_when_aec_inactive(monkeypatch):
    """When AEC is not active (TTS not playing or webrtc mode), Silero
    receives all input without pre-filtering."""
    sess, fake_vad = _make_session(monkeypatch, aec_active=False)

    quiet = (np.ones(480, dtype=np.float32) * 0.001 * 32768).astype("<i2")
    sess._feed_pcm_silero(quiet)

    fake_vad.accept_waveform.assert_called_once()


def test_silero_boundary_input_just_above_cutoff(monkeypatch):
    """Input right above the cutoff reaches Silero; just below does not."""
    sess, fake_vad = _make_session(monkeypatch, aec_active=True)
    # cutoff = 0.010 + 0.020 = 0.030 RMS. Use a sine-ish DC: amplitude
    # 0.035 → RMS 0.035 > 0.030 (passes).
    above = (np.ones(480, dtype=np.float32) * 0.035 * 32768).astype("<i2")
    sess._feed_pcm_silero(above)
    assert fake_vad.accept_waveform.call_count == 1

    # Below: amplitude 0.025 → RMS 0.025 < 0.030 (filtered).
    sess_below, fake_vad_below = _make_session(monkeypatch, aec_active=True)
    below = (np.ones(480, dtype=np.float32) * 0.025 * 32768).astype("<i2")
    sess_below._feed_pcm_silero(below)
    fake_vad_below.accept_waveform.assert_not_called()


def test_silero_returns_none_when_silero_unavailable(monkeypatch):
    """If sherpa_onnx init failed (_silero_vad is None), falls back to
    energy VAD — preserves original behavior."""
    import server

    monkeypatch.setattr(server, "AEC_MODE", "echo_window")
    sess = server.VoiceSession.__new__(server.VoiceSession)
    sess._silero_vad = None
    # Energy fallback path also needs these attrs — set safe values so it
    # short-circuits on threshold without completing a segment.
    sess._fsmn_vad = None
    sess.in_speech = False
    sess.speech_frames = 0
    sess._speech_buffer = []
    sess.tts_active = False
    sess.tts_last_chunk_ts = 0.0
    monkeypatch.setattr(server, "VAD_ENERGY_THRESHOLD", 999.0)
    monkeypatch.setattr(server, "VAD_MIN_SPEECH_MS", 9999)
    monkeypatch.setattr(server, "VAD_SILENCE_MS", 9999)

    result = sess._feed_pcm_silero(np.zeros(480, dtype="<i2"))
    assert result is None
