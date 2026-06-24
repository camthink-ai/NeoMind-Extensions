"""VAD backend implementations: energy, silero (sherpa-onnx), fsmn.

These classes implement the VADBackend Protocol from contracts.py.
VoiceSession in server.py still uses its internal _feed_pcm_* methods
for now — these classes are extracted for the Task 10 orchestrator.
"""
from __future__ import annotations

import logging
import os
from pathlib import Path

import numpy as np

from contracts import VadSegment

logger = logging.getLogger("voice-assistant.vad")

# Constants — kept in sync with server.py until Task 14c wires Profile.
# IMPORTANT: defaults must match server.py exactly so behavior is identical
# during the transition period.
SAMPLE_RATE = 16000
SILERO_VAD_MODEL_PATH = os.environ.get(
    "SILERO_VAD_MODEL_PATH",
    str(Path.home() / ".cache" / "sherpa-onnx" / "silero_vad.onnx"),
)
SILERO_VAD_THRESHOLD = float(os.environ.get("SILERO_VAD_THRESHOLD", "0.5"))
SILERO_VAD_MIN_SPEECH_MS = int(os.environ.get("SILERO_VAD_MIN_SPEECH_MS", "250"))
SILERO_VAD_SILENCE_MS = int(os.environ.get("SILERO_VAD_SILENCE_MS", "500"))


# ---------------------------------------------------------------------------
# Silero config — module-level singleton. Loaded once, shared across sessions.
# Each VoiceActivityDetector instance is created from this config.
# ---------------------------------------------------------------------------
_SILERO_VAD_CONFIG = None  # sherpa_onnx.VadModelConfig | None


def _ensure_silero_config():
    """Lazily load Silero VAD config, auto-download model if missing.

    Returns the VadModelConfig or None on failure. Idempotent — safe to
    call repeatedly; only the first call performs the load.

    This is a logic-preserving port of the inline init block that lived in
    server.py (originally L531-557). Constants, download URL, and the
    fallback-to-None-on-failure behavior are identical.
    """
    global _SILERO_VAD_CONFIG
    if _SILERO_VAD_CONFIG is not None:
        return _SILERO_VAD_CONFIG
    try:
        import sherpa_onnx

        silero_path = Path(SILERO_VAD_MODEL_PATH)
        if not silero_path.is_file():
            silero_path.parent.mkdir(parents=True, exist_ok=True)
            import urllib.request
            url = "https://github.com/snakers4/silero-vad/raw/master/files/silero_vad.onnx"
            logger.info("Downloading Silero VAD model → %s", silero_path)
            urllib.request.urlretrieve(url, silero_path)

        cfg = sherpa_onnx.VadModelConfig()
        cfg.silero_vad.model = str(silero_path)
        cfg.silero_vad.threshold = SILERO_VAD_THRESHOLD
        cfg.silero_vad.min_silence_duration = SILERO_VAD_SILENCE_MS / 1000.0
        cfg.silero_vad.min_speech_duration = SILERO_VAD_MIN_SPEECH_MS / 1000.0
        cfg.sample_rate = SAMPLE_RATE
        cfg.provider = "cpu"
        if not cfg.validate():
            raise RuntimeError("Silero VAD config invalid")
        _SILERO_VAD_CONFIG = cfg
        logger.info("Silero VAD config ready: %s", silero_path)
    except Exception as e:
        logger.warning("Silero VAD load failed: %s", e)
        _SILERO_VAD_CONFIG = None
    return _SILERO_VAD_CONFIG


class SileroVAD:
    """sherpa-onnx Silero v5 VAD. One instance per session.

    Implements the VADBackend Protocol from contracts.py. Wraps a
    sherpa_onnx.VoiceActivityDetector ring buffer; each feed() returns
    any completed speech segments.
    """

    def __init__(
        self,
        threshold: float = SILERO_VAD_THRESHOLD,
        min_speech_ms: int = SILERO_VAD_MIN_SPEECH_MS,
        silence_ms: int = SILERO_VAD_SILENCE_MS,
        sample_rate: int = SAMPLE_RATE,
    ):
        cfg = _ensure_silero_config()
        if cfg is None:
            raise RuntimeError("Silero VAD config unavailable")
        import sherpa_onnx
        self.threshold = threshold
        self.min_speech_ms = min_speech_ms
        self.silence_ms = silence_ms
        self._sample_rate = sample_rate
        self._vad = sherpa_onnx.VoiceActivityDetector(
            cfg, buffer_size_in_seconds=30,
        )

    @property
    def sample_rate(self) -> int:
        return self._sample_rate

    def feed(self, pcm_int16: bytes) -> list[VadSegment]:
        """Feed int16 LE PCM. Returns any completed speech segments.

        CRITICAL: read segment.samples BEFORE pop() — pop destroys the
        backing buffer in sherpa-onnx.
        """
        samples_int16 = np.frombuffer(pcm_int16, dtype=np.int16)
        audio = samples_int16.astype(np.float32) / 32768.0
        self._vad.accept_waveform(audio.tolist())
        segments: list[VadSegment] = []
        while not self._vad.empty():
            segment = self._vad.front
            samples = np.asarray(segment.samples, dtype=np.float32)  # read FIRST
            self._vad.pop()  # then pop (pop invalidates backing)
            if samples.size == 0:
                continue
            segments.append(VadSegment(
                samples=samples.tolist(),
                sample_rate=self._sample_rate,
                start_ms=0,
                end_ms=int(len(samples) / self._sample_rate * 1000),
            ))
        return segments

    def flush(self) -> list[VadSegment]:
        # sherpa-onnx VAD has no explicit flush; segments are returned via feed()
        return []


class EnergyVAD:
    """RMS energy threshold VAD (PoC legacy).

    Implements the VADBackend Protocol. Detects speech onset when the RMS
    energy of 30ms frames exceeds threshold for min_speech_ms, and emits a
    complete segment after silence_ms of sub-threshold frames.
    """

    def __init__(
        self,
        threshold: float = 0.015,
        min_speech_ms: int = 300,
        silence_ms: int = 500,
        sample_rate: int = SAMPLE_RATE,
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
        """Energy-based VAD. Returns complete utterance when silence detected."""
        samples_int16 = np.frombuffer(pcm_int16, dtype=np.int16)
        f = samples_int16.astype(np.float32) / 32768.0
        frame_len = int(self._sample_rate * 0.030)
        n_frames = len(f) // frame_len
        segments: list[VadSegment] = []
        for i in range(n_frames):
            fr = f[i * frame_len:(i + 1) * frame_len]
            rms = float(np.sqrt(np.mean(fr * fr)))
            is_speech = rms > self.threshold
            if is_speech:
                if not self.in_speech:
                    self.speech_frames += 1
                    if self.speech_frames >= self.min_speech_ms // 30:
                        self.in_speech = True
                        self.speech_audio = []
                        self.silence_frames = 0
                if self.in_speech:
                    self.speech_audio.append(fr)
                    self.silence_frames = 0
            else:
                if self.in_speech:
                    self.silence_frames += 1
                    self.speech_audio.append(fr * 0.0)  # zero-filled trailing
                    if self.silence_frames >= self.silence_ms // 30:
                        audio = (
                            np.concatenate(self.speech_audio)
                            if self.speech_audio
                            else np.zeros(0, dtype=np.float32)
                        )
                        segments.append(VadSegment(
                            samples=audio.tolist(),
                            sample_rate=self._sample_rate,
                            start_ms=0,
                            end_ms=int(len(audio) / self._sample_rate * 1000),
                        ))
                        self._reset()
                else:
                    self.speech_frames = 0
        return segments

    def flush(self) -> list[VadSegment]:
        return []

    def _reset(self) -> None:
        self.in_speech = False
        self.speech_audio = []
        self.silence_frames = 0
        self.speech_frames = 0


def get_silero_config():
    """Return the cached Silero VadModelConfig (loads on first call).

    Convenience accessor for server.py during the transition period —
    server.py imports this and uses it to populate its own
    _SILERO_VAD_CONFIG global.
    """
    return _ensure_silero_config()
