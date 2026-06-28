"""AEC backend adapters.

NoopAECBackend is the identity fallback used when:
  - profile.acoustic.aec is "none" or "echo_window" (no real AEC)
  - real-AEC library import or init fails (logged warning)

WebRtcAECBackend (Task 4) wraps webrtc-audio-processing-1.
"""
from __future__ import annotations

import numpy as np


class NoopAECBackend:
    """Identity AEC: returns mic unchanged. Used as fallback."""

    def init(self, sample_rate: int) -> bool:
        return True

    def process_capture(self, mic_pcm: np.ndarray, reference_pcm: np.ndarray) -> np.ndarray:
        return mic_pcm

    def close(self) -> None:
        pass


# WebRTC AudioProcessingModule is imported lazily so the module loads
# cleanly when webrtc-audio-processing is not installed (factory will
# then fall back to NoopAECBackend).
_WEBRTC_APM_CLASS = None  # patched by tests; resolved lazily on first init


def _resolve_webrtc_apm_class():
    """Return webrtc_audio_processing.AudioProcessingModule, or None if unavailable."""
    global _WEBRTC_APM_CLASS
    if _WEBRTC_APM_CLASS is not None:
        return _WEBRTC_APM_CLASS
    try:
        from webrtc_audio_processing import AudioProcessingModule
        _WEBRTC_APM_CLASS = AudioProcessingModule
        return AudioProcessingModule
    except Exception:
        return None


class WebRtcAECBackend:
    """Adapts webrtc_audio_processing.AudioProcessingModule to the AECBackend Protocol.

    WebRTC's APM operates on fixed 10ms frames at 16kHz mono int16 (160 samples
    = 320 bytes per frame). process_capture chunks input into 10ms frames and
    for each frame: feeds reference via process_reverse_stream, then feeds mic
    via process_stream, capturing the cleaned output.
    """

    FRAME_SAMPLES = 160  # 10ms @ 16kHz
    FRAME_BYTES = 320    # int16 mono

    def __init__(self, aec_type: int = 2) -> None:
        # aec_type: 0=off, 1=AECM (mobile, low-complexity), 2=AEC (full)
        self._aec_type = aec_type
        self._apm = None
        self._sample_rate = None

    def init(self, sample_rate: int) -> bool:
        cls = _resolve_webrtc_apm_class()
        if cls is None:
            return False
        try:
            self._apm = cls(aec_type=self._aec_type)
            self._apm.set_stream_format(sample_rate, 1)
            self._apm.set_reverse_stream_format(sample_rate, 1)
            # Enable AEC at level 1 (WebRTC convention: 0=off, 1=on)
            self._apm.set_aec_level(1)
            self._sample_rate = sample_rate
            return True
        except Exception:
            self._apm = None
            return False

    def process_capture(self, mic_pcm: np.ndarray, reference_pcm: np.ndarray) -> np.ndarray:
        if self._apm is None:
            return mic_pcm
        mic_bytes = bytes(mic_pcm.tobytes())
        ref_bytes = bytes(reference_pcm.tobytes())
        out_chunks = []
        ref_pos = 0
        for cap_pos in range(0, len(mic_bytes), self.FRAME_BYTES):
            cap_chunk = mic_bytes[cap_pos:cap_pos + self.FRAME_BYTES]
            if len(cap_chunk) < self.FRAME_BYTES:
                cap_chunk = cap_chunk + b"\0" * (self.FRAME_BYTES - len(cap_chunk))
            ref_chunk = ref_bytes[ref_pos:ref_pos + self.FRAME_BYTES]
            if len(ref_chunk) < self.FRAME_BYTES:
                ref_chunk = ref_chunk + b"\0" * (self.FRAME_BYTES - len(ref_chunk))
            ref_pos += self.FRAME_BYTES
            # Per-frame delay convention: 10ms (160 samples) of round-trip delay
            try:
                self._apm.set_system_delay(self.FRAME_SAMPLES)
                self._apm.process_reverse_stream(ref_chunk)
                cleaned = self._apm.process_stream(cap_chunk)
                out_chunks.append(cleaned)
            except Exception:
                # If a frame fails, fall back to passthrough for this frame
                out_chunks.append(cap_chunk)
        return np.frombuffer(b"".join(out_chunks), dtype="<i2")[:len(mic_pcm)]

    def close(self) -> None:
        self._apm = None
