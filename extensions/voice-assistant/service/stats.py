"""Rolling percentile for latency KPIs. O(1) observe, O(N log N) percentile."""
from __future__ import annotations

from collections import deque


class RollingPercentile:
    """Fixed-size sliding window of float samples."""

    def __init__(self, window: int = 100):
        self.samples: deque[float] = deque(maxlen=window)

    def observe(self, value_ms: float) -> None:
        self.samples.append(value_ms)

    def percentile(self, p: float) -> float:
        if not self.samples:
            return 0.0
        sorted_samples = sorted(self.samples)
        idx = int(len(sorted_samples) * p / 100)
        idx = min(idx, len(sorted_samples) - 1)
        return sorted_samples[idx]
