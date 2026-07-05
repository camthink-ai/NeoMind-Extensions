"""Echo Return Loss Enhancement (ERLE) tracker for AEC residual echo gating.

During TTS playback the AEC removes the speaker echo from the mic signal.
When AEC works well, the post-AEC signal has much lower energy than the
pre-AEC signal (high ERLE). When AEC fails to converge, residual echo
remains in the post-AEC signal — which can falsely trip VAD and cause
spurious barge-in.

This module provides a lightweight rolling-statistics tracker that the
voice session uses to:

1. Expose ERLE telemetry (validates AEC effectiveness).
2. Gate barge-in: when reference signal energy dominates post-AEC mic
   energy, detected "speech" is likely residual echo rather than user
   speech, and the segment is suppressed.

The tracker is updated per PCM frame during the AEC echo window only.
Outside the echo window the values go stale and callers should clear
them via :meth:`reset`.
"""
from __future__ import annotations

import collections
import logging
import math
import statistics

logger = logging.getLogger("voice-assistant.erle")


class ErleTracker:
    """Rolling ERLE + reference-dominance tracker.

    Updated per PCM frame during echo-window periods. Callers query
    :meth:`ref_dominance_ratio` to decide whether a VAD-detected speech
    segment is likely residual echo.

    A ratio >1.0 means the reference (TTS playback) signal is louder than
    what the mic picked up after AEC processing — a strong indicator that
    the detected signal is echo leak rather than genuine user speech.
    """

    def __init__(self, window: int = 50) -> None:
        self._window = window
        self._mic_rms_buf: collections.deque[float] = collections.deque(maxlen=window)
        self._post_rms_buf: collections.deque[float] = collections.deque(maxlen=window)
        self._ref_rms_buf: collections.deque[float] = collections.deque(maxlen=window)
        # Incremented each time a VAD segment is suppressed as residual echo.
        self.rejected_barge_ins: int = 0

    def update(self, mic_rms: float, post_rms: float, ref_rms: float) -> None:
        """Push one frame's RMS triple.

        All three values are linear RMS amplitudes in the float-[-1, 1]
        range (i.e. int16 samples divided by 32768). Negatives or NaN are
        silently dropped — callers should not pass them, but the guard
        keeps telemetry robust against pathological AEC output.
        """
        if mic_rms <= 0.0 or post_rms < 0.0 or ref_rms < 0.0:
            return
        if math.isnan(mic_rms) or math.isnan(post_rms) or math.isnan(ref_rms):
            return
        self._mic_rms_buf.append(mic_rms)
        self._post_rms_buf.append(post_rms)
        self._ref_rms_buf.append(ref_rms)

    def reset(self) -> None:
        """Clear history.

        Should be called when the AEC echo window ends so stale samples
        don't bleed into the next playback period.
        """
        self._mic_rms_buf.clear()
        self._post_rms_buf.clear()
        self._ref_rms_buf.clear()

    def instant_erle_db(self) -> float:
        """ERLE in dB based on rolling means.

        High (>15 dB): AEC is removing most of the reference energy.
        Low (<5 dB): either AEC is failing to converge, or the user is
        speaking over TTS (double-talk — the mic energy is genuine user
        speech that AEC rightly does not cancel). ERLE alone cannot
        distinguish these; :meth:`ref_dominance_ratio` is the better
        signal for the barge-in gate.

        Returns 0.0 when there are no samples.
        """
        if not self._mic_rms_buf:
            return 0.0
        mic = statistics.fmean(self._mic_rms_buf)
        post = statistics.fmean(self._post_rms_buf)
        if post <= 1e-8:
            # Near-perfect cancellation — clamp at a sane ceiling.
            return 60.0
        if mic <= 1e-8:
            return 0.0
        return 10.0 * math.log10((mic * mic) / (post * post))

    def ref_dominance_ratio(self) -> float:
        """mean(ref_rms) / mean(post_rms).

        >1.0 → reference signal dominates post-AEC mic → residual echo
        likely. <1.0 → mic exceeds reference → genuine user speech
        likely. Returns 0.0 when there are no samples or when the
        post-AEC energy is effectively zero (no useful ratio).
        """
        if not self._post_rms_buf:
            return 0.0
        ref = statistics.fmean(self._ref_rms_buf)
        post = statistics.fmean(self._post_rms_buf)
        if post <= 1e-8:
            return 0.0
        return ref / post

    def has_samples(self) -> bool:
        return bool(self._mic_rms_buf)

    def snapshot(self) -> dict[str, float | int]:
        """Operator-facing summary for the /measure endpoint."""
        return {
            "erle_db": round(self.instant_erle_db(), 2),
            "ref_dominance": round(self.ref_dominance_ratio(), 3),
            "rejected_barge_ins": self.rejected_barge_ins,
            "samples": len(self._mic_rms_buf),
        }
