#!/usr/bin/env python3
"""
Voice turn latency measurement (v2 orchestrator protocol).

Connects to the voice-assistant orchestrator WS, feeds synthetic speech
PCM loud enough to trigger energy VAD, then records the timeline:

    ws_connect → ready → first_pcm_sent → asr_start → transcript →
    tts_start → first_binary PCM → tts_end → stop

Key user-perceived latency numbers:
  - ASR done → first_binary  : how long after user stops talking they
                                hear the first audio response.
  - first_binary - tts_start : TTS synthesize latency (full_synthesize mode).
  - turn_total               : full single-turn wall-clock.

Also fetches /measure afterward to pull the server-side RollingPercentile
KPIs (asr_complete_ms, llm_ttfb_ms, tts_first_chunk_ms, first_audio_out_ms,
full_turn_ms) accumulated across all sessions.

Run:
    # 1. orchestrator on :9384
    python server.py --host 127.0.0.1 --port 9384 &

    # 2. measure
    python measure_first_sentence.py --n 5
"""
from __future__ import annotations

import argparse
import asyncio
import json
import time
import urllib.request
from typing import Optional

import numpy as np
import websockets

SAMPLE_RATE = 16000


def synth_speech_burst(duration_s: float, freq: float = 440.0) -> bytes:
    """Sine tone as int16 LE PCM at 16kHz mono — emulates a loud utterance."""
    n = int(SAMPLE_RATE * duration_s)
    t = np.linspace(0, duration_s, n, endpoint=False)
    f = 0.3 * np.sin(2 * np.pi * freq * t)
    return (f * 32767).astype("<i2").tobytes()


def synth_silence(duration_s: float) -> bytes:
    n = int(SAMPLE_RATE * duration_s)
    return np.zeros(n, dtype="<i2").tobytes()


async def measure(url: str) -> dict:
    timeline = {
        "ws_connect": None,
        "ready": None,
        "first_pcm_sent": None,
        "asr_start": None,
        "asr_done": None,
        "tts_start": None,
        "first_binary": None,
        "tts_end": None,
        "stop": None,
        "transcript": "",
        "tts_end_payload": None,
        "binary_chunk_count": 0,
        "total_binary_bytes": 0,
        "binary_chunk_ts": [],
    }

    t0 = time.perf_counter()
    async with websockets.connect(url, max_size=None) as ws:
        timeline["ws_connect"] = (time.perf_counter() - t0) * 1000

        await ws.send(json.dumps({"type": "start"}))

        # Drain until ready
        async for msg in ws:
            try:
                obj = json.loads(msg)
            except Exception:
                continue
            if obj.get("type") == "ready":
                timeline["ready"] = (time.perf_counter() - t0) * 1000
                break

        # 1.2s loud tone (triggers speech-start) + 0.8s silence (triggers
        # speech-end). Send in 100ms frames at 2× realtime to mimic stream.
        speech = synth_speech_burst(1.2)
        silence = synth_silence(0.8)
        frame_size = int(SAMPLE_RATE * 0.1) * 2
        for pcm in (speech, silence):
            for off in range(0, len(pcm), frame_size):
                await ws.send(pcm[off:off + frame_size])
                await asyncio.sleep(0.05)
        timeline["first_pcm_sent"] = (time.perf_counter() - t0) * 1000

        async for msg in ws:
            now = time.perf_counter()
            if isinstance(msg, (bytes, bytearray)):
                ts_ms = (now - t0) * 1000
                if timeline["first_binary"] is None:
                    timeline["first_binary"] = ts_ms
                timeline["binary_chunk_count"] += 1
                timeline["total_binary_bytes"] += len(msg)
                timeline["binary_chunk_ts"].append(ts_ms)
                continue
            try:
                obj = json.loads(msg)
            except Exception:
                continue
            t = obj.get("type")
            ts = (now - t0) * 1000
            if t == "asr_start":
                timeline["asr_start"] = ts
            elif t == "transcript":
                timeline["asr_done"] = ts
                timeline["transcript"] = obj.get("text", "")
            elif t == "tts_start":
                timeline["tts_start"] = ts
            elif t == "tts_end":
                timeline["tts_end"] = ts
                timeline["tts_end_payload"] = obj.get("metrics", {})
            elif t == "stop":
                timeline["stop"] = ts
                break
            elif t in ("error", "barge_in", "skip"):
                timeline.setdefault("terminal", {})[t] = ts
                break

    return timeline


def fetch_server_metrics(http_base: str) -> Optional[dict]:
    """Pull /measure KPI snapshot (server-side RollingPercentile)."""
    try:
        with urllib.request.urlopen(f"{http_base}/measure", timeout=2) as r:
            return json.loads(r.read())
    except Exception as e:
        print(f"  (could not fetch /measure: {e})")
        return None


def fmt(v):
    return f"{v:.0f}ms" if v is not None else "—"


def print_report(tl: dict, server_metrics: Optional[dict]) -> None:
    print()
    print("=" * 64)
    print(f"Transcript: \"{tl.get('transcript', '')}\"")
    print()
    print("Client-side timeline (ms from ws connect):")
    print(f"  ws_connect:        {fmt(tl['ws_connect'])}")
    print(f"  ready:             {fmt(tl['ready'])}")
    print(f"  first_pcm_sent:    {fmt(tl['first_pcm_sent'])}")
    print(f"  asr_start:         {fmt(tl['asr_start'])}")
    print(f"  asr_done:          {fmt(tl['asr_done'])}")
    print(f"  tts_start:         {fmt(tl['tts_start'])}")
    print(f"  first_binary:      {fmt(tl['first_binary'])}")
    print(f"  tts_end:           {fmt(tl['tts_end'])}")
    print(f"  stop:              {fmt(tl['stop'])}")
    print()
    print("Key user-perceived gaps:")
    pairs = [
        ("ASR done → first audio",   tl["asr_done"],    tl["first_binary"]),
        ("VAD end → ASR done",       tl["first_pcm_sent"], tl["asr_done"]),
        ("ASR done → TTS start",     tl["asr_done"],    tl["tts_start"]),
        ("TTS start → first binary", tl["tts_start"],   tl["first_binary"]),
        ("Turn total (pcm→stop)",    tl["first_pcm_sent"], tl["stop"]),
    ]
    for label, a, b in pairs:
        if a is not None and b is not None:
            print(f"  {label:<28} {b - a:>6.0f}ms")
    print()
    print(f"Binary PCM chunks: {tl['binary_chunk_count']}  "
          f"({tl['total_binary_bytes']} bytes, "
          f"~{tl['total_binary_bytes'] / 2 / SAMPLE_RATE * 1000:.0f}ms of audio)")

    # Inter-chunk gap — for v2 (single full_synthesize call) this should be
    # tiny and uniform. Large gaps indicate the server is buffering or
    # stalling between sends.
    chunk_ts = tl.get("binary_chunk_ts", [])
    if len(chunk_ts) >= 2:
        gaps = [chunk_ts[i + 1] - chunk_ts[i] for i in range(len(chunk_ts) - 1)]
        print(f"Inter-chunk PCM gap: max={max(gaps):.0f}ms  "
              f"mean={sum(gaps) / len(gaps):.0f}ms  "
              f"p95={sorted(gaps)[int(len(gaps) * 0.95) - 1]:.0f}ms")

    if tl.get("tts_end_payload"):
        p = tl["tts_end_payload"]
        print()
        print("Server-reported metrics (in tts_end frame):")
        for k in ("total_ms", "tts_first_chunk_ms", "asr_ms"):
            if k in p:
                print(f"  {k:<22} {p[k]:.0f}ms")

    if server_metrics:
        print()
        print("Server-side RollingPercentile (/measure):")
        turns = server_metrics.get("turn_count", "?")
        barges = server_metrics.get("barge_in_count", 0)
        print(f"  turn_count={turns}  barge_in_count={barges}")
        for kpi in ("asr_complete_ms", "llm_ttfb_ms", "tts_first_chunk_ms",
                    "first_audio_out_ms", "full_turn_ms"):
            v = server_metrics.get(kpi)
            if isinstance(v, dict):
                print(f"  {kpi:<22} p50={v.get('p50', 0):.0f}ms  "
                      f"p95={v.get('p95', 0):.0f}ms  "
                      f"min={v.get('min', 0):.0f}ms  "
                      f"max={v.get('max', 0):.0f}ms")
    print("=" * 64)


async def main():
    p = argparse.ArgumentParser()
    p.add_argument("--url", default="ws://127.0.0.1:9384/ws")
    p.add_argument("--http", default="http://127.0.0.1:9384")
    p.add_argument("--n", type=int, default=1, help="iterations")
    args = p.parse_args()
    for i in range(args.n):
        if i > 0:
            print("\n\n--- next iteration ---")
            await asyncio.sleep(2)
        tl = await measure(args.url)
        sm = fetch_server_metrics(args.http) if i == args.n - 1 else None
        print_report(tl, sm)


if __name__ == "__main__":
    asyncio.run(main())
