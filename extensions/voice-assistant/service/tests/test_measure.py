"""/measure endpoint returns aggregated telemetry KPI snapshot."""
from __future__ import annotations

from unittest.mock import patch

import pytest
from fastapi.testclient import TestClient


def test_measure_returns_kpi_snapshot():
    """POST /measure returns turn_count, target_ms, target_met, and KPI snapshot."""
    canned_snapshot = {
        "first_audio_out_ms": {"p50": 950.0, "p95": 1100.0, "min": 900.0, "max": 1200.0},
        "asr_complete_ms": {"p50": 200.0, "p95": 250.0, "min": 180.0, "max": 280.0},
        "llm_ttfb_ms": {"p50": 150.0, "p95": 180.0, "min": 140.0, "max": 200.0},
    }
    with patch("server._telemetry") as mock_t:
        mock_t.snapshot.return_value = canned_snapshot
        mock_t.turn_count = 5
        mock_t.barge_in_count = 1
        # Patch latency target via the profile too
        with patch("server._profile") as mock_p:
            mock_p.latency_target_ms = 1200
            from server import app
            client = TestClient(app)
            resp = client.post("/measure", json={})
    assert resp.status_code == 200
    data = resp.json()
    assert data["turn_count"] == 5
    assert data["barge_in_count"] == 1
    assert data["target_ms"] == 1200
    assert data["target_met"] is True  # 950 < 1200
    assert "first_audio_out_ms" in data
    assert data["first_audio_out_ms"]["p50"] == 950.0


def test_measure_target_not_met_when_p50_exceeds():
    """target_met is False when first_audio_out_ms.p50 > latency_target_ms."""
    canned = {
        "first_audio_out_ms": {"p50": 2000.0, "p95": 2500.0, "min": 1800.0, "max": 3000.0},
    }
    with patch("server._telemetry") as mock_t:
        mock_t.snapshot.return_value = canned
        mock_t.turn_count = 3
        mock_t.barge_in_count = 0
        with patch("server._profile") as mock_p:
            mock_p.latency_target_ms = 1200
            from server import app
            client = TestClient(app)
            resp = client.post("/measure", json={})
    assert resp.status_code == 200
    data = resp.json()
    assert data["target_met"] is False


def test_measure_empty_telemetry_returns_target_met_false():
    """Empty telemetry (no first_audio_out_ms observations) -> target_met False."""
    with patch("server._telemetry") as mock_t:
        mock_t.snapshot.return_value = {}  # no KPIs observed yet
        mock_t.turn_count = 0
        mock_t.barge_in_count = 0
        with patch("server._profile") as mock_p:
            mock_p.latency_target_ms = 1200
            from server import app
            client = TestClient(app)
            resp = client.post("/measure", json={})
    assert resp.status_code == 200
    data = resp.json()
    assert data["turn_count"] == 0
    assert data["target_met"] is False  # no observations -> not met
