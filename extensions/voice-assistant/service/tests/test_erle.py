"""Tests for the ErleTracker module."""
from __future__ import annotations

import os
import sys
from pathlib import Path

# Make service/ importable when run from anywhere
SERVICE_DIR = Path(__file__).resolve().parent.parent
if str(SERVICE_DIR) not in sys.path:
    sys.path.insert(0, str(SERVICE_DIR))

from erle import ErleTracker  # noqa: E402


def test_empty_state_returns_zeros():
    t = ErleTracker()
    assert not t.has_samples()
    assert t.instant_erle_db() == 0.0
    assert t.ref_dominance_ratio() == 0.0
    snap = t.snapshot()
    assert snap["samples"] == 0
    assert snap["rejected_barge_ins"] == 0
    assert snap["erle_db"] == 0.0
    assert snap["ref_dominance"] == 0.0


def test_high_erle_when_post_aec_much_smaller():
    """Healthy AEC: post is 1/10 of mic → ~20 dB ERLE."""
    t = ErleTracker()
    t.update(mic_rms=0.10, post_rms=0.01, ref_rms=0.05)
    erle = t.instant_erle_db()
    # 10 * log10((0.1^2) / (0.01^2)) = 10 * log10(100) = 20 dB
    assert abs(erle - 20.0) < 0.1


def test_low_erle_when_post_aec_similar_to_mic():
    """Failing AEC or double-talk: post ~= mic → ~0 dB ERLE."""
    t = ErleTracker()
    t.update(mic_rms=0.05, post_rms=0.045, ref_rms=0.04)
    erle = t.instant_erle_db()
    assert erle < 1.5  # essentially no cancellation


def test_perfect_cancellation_clamped():
    """post_rms → 0 should clamp at the ceiling (60 dB), not divide-by-zero."""
    t = ErleTracker()
    t.update(mic_rms=0.10, post_rms=1e-9, ref_rms=0.05)
    assert t.instant_erle_db() == 60.0


def test_ref_dominance_gt1_when_ref_louder_than_post():
    """Reference dominates post-AEC mic → residual echo likely."""
    t = ErleTracker()
    t.update(mic_rms=0.05, post_rms=0.01, ref_rms=0.08)
    # ref/post = 0.08 / 0.01 = 8.0
    assert abs(t.ref_dominance_ratio() - 8.0) < 0.01


def test_ref_dominance_lt1_when_post_louder_than_ref():
    """Post-AEC mic exceeds reference → genuine user speech likely."""
    t = ErleTracker()
    t.update(mic_rms=0.20, post_rms=0.15, ref_rms=0.04)
    # ref/post = 0.04 / 0.15 ≈ 0.267
    assert t.ref_dominance_ratio() < 1.0
    assert abs(t.ref_dominance_ratio() - (0.04 / 0.15)) < 0.01


def test_rolling_window_averages_multiple_samples():
    t = ErleTracker(window=10)
    # Two frames with different ERLE; verify mean is used.
    t.update(mic_rms=0.10, post_rms=0.01, ref_rms=0.05)  # 20 dB
    t.update(mic_rms=0.10, post_rms=0.0316, ref_rms=0.05)  # 10 dB
    # mean(mic) = 0.1, mean(post) = (0.01 + 0.0316)/2 = 0.0208
    # ERLE = 10*log10(0.1^2 / 0.0208^2) = 10*log10(0.01 / 0.0004326)
    #      ≈ 10*log10(23.1) ≈ 13.64 dB
    erle = t.instant_erle_db()
    assert 13.0 < erle < 14.5


def test_window_eviction_drops_old_samples():
    t = ErleTracker(window=3)
    for _ in range(5):
        t.update(mic_rms=0.10, post_rms=0.01, ref_rms=0.05)
    # All 5 should give 20 dB ERLE; window just caps at 3.
    assert t.snapshot()["samples"] == 3
    assert abs(t.instant_erle_db() - 20.0) < 0.1


def test_reset_clears_history_but_keeps_counter():
    t = ErleTracker()
    t.update(mic_rms=0.1, post_rms=0.01, ref_rms=0.05)
    t.rejected_barge_ins = 7
    t.reset()
    assert not t.has_samples()
    assert t.instant_erle_db() == 0.0
    # Counter survives reset — it's a running total, not window state.
    assert t.rejected_barge_ins == 7


def test_invalid_inputs_silently_dropped():
    t = ErleTracker()
    t.update(mic_rms=-0.1, post_rms=0.01, ref_rms=0.05)  # negative mic
    t.update(mic_rms=0.1, post_rms=float("nan"), ref_rms=0.05)  # NaN post
    t.update(mic_rms=0.0, post_rms=0.0, ref_rms=0.0)  # zero
    assert not t.has_samples()


def test_snapshot_includes_all_fields():
    t = ErleTracker()
    t.update(mic_rms=0.10, post_rms=0.01, ref_rms=0.05)
    t.rejected_barge_ins = 3
    snap = t.snapshot()
    assert set(snap.keys()) == {"erle_db", "ref_dominance", "rejected_barge_ins", "samples"}
    assert snap["samples"] == 1
    assert snap["rejected_barge_ins"] == 3
    assert isinstance(snap["erle_db"], float)
    assert isinstance(snap["ref_dominance"], float)


def test_gate_scenario_residual_echo():
    """End-to-end: AEC failed, ref dominates post-AEC → ratio > 1.5
    would suppress barge-in."""
    t = ErleTracker()
    # Simulate 30 frames of mostly-residual-echo: ref loud, post-AEC
    # still substantial (AEC only removed a little).
    for _ in range(30):
        t.update(mic_rms=0.10, post_rms=0.04, ref_rms=0.08)
    # ref_dominance = 0.08 / 0.04 = 2.0 → exceeds 1.5 threshold
    assert t.ref_dominance_ratio() > 1.5


def test_gate_scenario_genuine_speech():
    """End-to-end: user speaks louder than TTS → mic dominates → ratio < 1."""
    t = ErleTracker()
    for _ in range(30):
        t.update(mic_rms=0.30, post_rms=0.25, ref_rms=0.05)
    assert t.ref_dominance_ratio() < 1.0


if __name__ == "__main__":
    # Allow `python test_erle.py` without pytest for quick smoke checks.
    import inspect
    funcs = [v for k, v in sorted(globals().items())
             if k.startswith("test_") and callable(v)]
    passed = 0
    for fn in funcs:
        try:
            fn()
            passed += 1
            print(f"PASS  {fn.__name__}")
        except AssertionError as e:
            print(f"FAIL  {fn.__name__}: {e}")
        except Exception as e:
            print(f"ERROR {fn.__name__}: {type(e).__name__}: {e}")
    print(f"\n{passed}/{len(funcs)} tests passed")
    sys.exit(0 if passed == len(funcs) else 1)
