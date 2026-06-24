"""Telemetry KPI tracking tests."""
from __future__ import annotations

from telemetry import Telemetry


def test_observe_first_audio_out():
    t = Telemetry()
    t.observe("first_audio_out_ms", 920.0)
    assert t.percentile("first_audio_out_ms", 50) == 920.0


def test_observe_multiple_kpis():
    t = Telemetry()
    for v in [100, 200, 300]:
        t.observe("asr_complete_ms", float(v))
    assert t.percentile("asr_complete_ms", 50) == 200.0


def test_unknown_kpi_returns_zero():
    t = Telemetry()
    assert t.percentile("nonexistent", 50) == 0.0


def test_snapshot_returns_all_kpis():
    t = Telemetry()
    t.observe("first_audio_out_ms", 950.0)
    t.observe("barge_in_to_silence_ms", 150.0)
    snap = t.snapshot()
    assert "first_audio_out_ms" in snap
    assert "barge_in_to_silence_ms" in snap
    assert snap["first_audio_out_ms"]["p50"] == 950.0
    assert snap["barge_in_to_silence_ms"]["p50"] == 150.0


def test_turn_count_increments():
    t = Telemetry()
    assert t.turn_count == 0
    t.increment_turns()
    t.increment_turns()
    assert t.turn_count == 2
