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
