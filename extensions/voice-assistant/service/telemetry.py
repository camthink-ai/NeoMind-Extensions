"""Latency KPI tracking via rolling percentiles."""
from __future__ import annotations

import logging
import os

from stats import RollingPercentile

logger = logging.getLogger("voice-assistant.telemetry")

# 7 KPIs per spec Section 6.1
KPI_NAMES = (
    "asr_complete_ms",
    "llm_ttfb_ms",
    "tts_first_chunk_ms",
    "first_audio_out_ms",
    "full_turn_ms",
    "barge_in_to_silence_ms",
    "barge_in_cancel_ack_ms",
)


class Telemetry:
    """Tracks latency KPIs across the last N turns."""

    def __init__(self, window: int = 100):
        self._kpis: dict[str, RollingPercentile] = {
            name: RollingPercentile(window) for name in KPI_NAMES
        }
        self.turn_count = 0
        self.barge_in_count = 0
        self._otel_enabled = os.environ.get("VOICE_ASSISTANT_TRACE", "") == "1"
        self.tracer = None
        if self._otel_enabled:
            self._setup_otel()

    def _setup_otel(self) -> None:
        try:
            from opentelemetry import trace
            from opentelemetry.sdk.trace import TracerProvider
            from opentelemetry.sdk.trace.export import ConsoleSpanExporter, BatchSpanProcessor
            provider = TracerProvider()
            provider.add_span_processor(BatchSpanProcessor(ConsoleSpanExporter()))
            trace.set_tracer_provider(provider)
            self.tracer = trace.get_tracer("voice-assistant")
        except ImportError:
            logger.warning("opentelemetry-sdk not installed; trace disabled")
            self._otel_enabled = False
            self.tracer = None

    def observe(self, kpi: str, value_ms: float) -> None:
        if kpi in self._kpis:
            self._kpis[kpi].observe(value_ms)

    def percentile(self, kpi: str, p: float) -> float:
        rp = self._kpis.get(kpi)
        return rp.percentile(p) if rp else 0.0

    def increment_turns(self) -> None:
        self.turn_count += 1

    def increment_barge_ins(self) -> None:
        self.barge_in_count += 1

    def snapshot(self) -> dict[str, dict[str, float]]:
        """Return {kpi_name: {p50, p95, min, max}} for all KPIs with samples."""
        result = {}
        for name, rp in self._kpis.items():
            if rp.samples:
                result[name] = {
                    "p50": rp.percentile(50),
                    "p95": rp.percentile(95),
                    "min": min(rp.samples),
                    "max": max(rp.samples),
                }
        return result
