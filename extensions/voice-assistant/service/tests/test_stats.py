"""RollingPercentile keeps last N samples and reports percentiles."""
from __future__ import annotations

from stats import RollingPercentile


def test_empty_returns_zero():
    rp = RollingPercentile(window=10)
    assert rp.percentile(50) == 0.0


def test_single_sample():
    rp = RollingPercentile(window=10)
    rp.observe(100.0)
    assert rp.percentile(50) == 100.0


def test_multiple_samples_p50():
    rp = RollingPercentile(window=100)
    for v in [10, 20, 30, 40, 50, 60, 70, 80, 90, 100]:
        rp.observe(float(v))
    # sorted: [10..100], p50 index = int(10*0.5) = 5 → 60
    assert rp.percentile(50) == 60.0


def test_window_eviction():
    rp = RollingPercentile(window=3)
    rp.observe(100.0)
    rp.observe(200.0)
    rp.observe(300.0)
    rp.observe(400.0)  # evicts 100
    # sorted: [200, 300, 400], p50 index = int(3*0.5) = 1 → 300
    assert rp.percentile(50) == 300.0


def test_p95_index_clamped():
    rp = RollingPercentile(window=5)
    for v in [1, 2, 3, 4, 5]:
        rp.observe(float(v))
    # sorted: [1,2,3,4,5], p95 index = int(5*0.95) = 4, clamped to 4 → 5
    assert rp.percentile(95) == 5.0
