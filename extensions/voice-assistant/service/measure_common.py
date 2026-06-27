"""Shared helpers for voice-assistant WS-based E2E measurement scripts.

Used by:
  - measure_bi_stream_e2e.py  (Phase 2 bi-streaming acceptance)
  - measure_neomind_e2e.py   (NeoMind chat WS integration acceptance)

Keeps WS protocol handling in one place to avoid drift between the two
acceptance harnesses.
"""
from __future__ import annotations

import asyncio
import io
import json
import time
import urllib.request
import wave

import numpy as np
import soundfile as sf
import websockets

SAMPLE_RATE = 16000
# 100ms chunks — server.py's energy VAD processes 30ms frames internally,
# chunks smaller than 30ms (480 samples) get silently dropped by the frame
# loop in _feed_pcm_energy. 100ms gives ~3 VAD frames per chunk.
CHUNK_BYTES = SAMPLE_RATE * 2 * 100 // 1000

# WS event types that terminate a turn read loop.
TURN_TERMINATORS = ("stop", "error", "skip")


def load_speech(path: str) -> bytes:
    """Load any audio file as 16kHz mono int16 LE PCM bytes."""
    data, sr = sf.read(path, dtype="float32", always_2d=False)
    if data.ndim > 1:
        data = data.mean(axis=1)
    if sr != SAMPLE_RATE:
        n_out = int(round(len(data) * SAMPLE_RATE / sr))
        idx = np.linspace(0, len(data) - 1, n_out)
        data = np.interp(idx, np.arange(len(data)), data).astype(np.float32)
    pcm = (np.clip(data, -1, 1) * 32767).astype("<i2")
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SAMPLE_RATE)
        w.writeframes(pcm.tobytes())
    return pcm.tobytes()


async def run_one_turn(
    ws_url: str,
    pcm: bytes,
    turn_timeout: float = 30.0,
    on_event: dict | None = None,
) -> dict:
    """Send PCM to orchestrator WS, collect events, return timing metrics.

    Timing markers:
      asr_done → tts_start      : orchestrator-side TTS engagement (real bi-stream start)
      asr_done → first_tts_pcm  : first PCM AFTER tts_start frame (excludes stage fillers)
      asr_done → last_pcm       : last PCM of the turn (includes fillers)

    Optional ``on_event`` callback is invoked with every parsed JSON message
    (and with the special ``"_binary"`` marker for PCM frames) so callers
    can layer their own domain-specific observation (e.g. NeoMind error
    classification) on top of the standard timing collection.
    """
    t_start = time.perf_counter()
    t_asr_done: float | None = None
    t_tts_start: float | None = None
    t_first_tts_pcm: float | None = None
    t_last_pcm: float | None = None
    llm_sentence_count = 0
    tts_chunk_count = 0
    # Total binary chunks, and chunks seen AFTER tts_start (real TTS PCM).
    post_tts_pcm_chunks = 0
    error_events: list[dict] = []

    async with websockets.connect(ws_url, max_size=2 ** 24) as ws:
        await ws.send(json.dumps({
            "type": "start", "sample_rate": SAMPLE_RATE, "language": "auto",
        }))

        # Stream the audio in 100ms chunks.
        async def feed_audio():
            for i in range(0, len(pcm), CHUNK_BYTES):
                await ws.send(pcm[i:i + CHUNK_BYTES])
                await asyncio.sleep(0.100)
            # Send a tail of silence to flush VAD (silence_ms=500).
            silence = b"\x00\x00" * SAMPLE_RATE * 2  # 2s silence
            for i in range(0, len(silence), CHUNK_BYTES):
                await ws.send(silence[i:i + CHUNK_BYTES])
                await asyncio.sleep(0.100)

        feed_task = asyncio.create_task(feed_audio())

        try:
            async for raw in ws:
                if isinstance(raw, bytes):
                    tts_chunk_count += 1
                    now = time.perf_counter()
                    t_last_pcm = now
                    if t_tts_start is not None:
                        post_tts_pcm_chunks += 1
                        if t_first_tts_pcm is None:
                            t_first_tts_pcm = now
                    if on_event is not None:
                        on_event({"_binary": True, "t": now})
                    continue
                try:
                    msg = json.loads(raw)
                except json.JSONDecodeError:
                    continue
                mtype = msg.get("type")
                if on_event is not None:
                    on_event(msg)
                if mtype == "transcript":
                    t_asr_done = time.perf_counter()
                elif mtype == "tts_start":
                    t_tts_start = time.perf_counter()
                elif mtype == "llm_sentence":
                    llm_sentence_count += 1
                elif mtype == "error":
                    error_events.append(msg)
                if mtype in TURN_TERMINATORS:
                    break
        finally:
            if not feed_task.done():
                feed_task.cancel()

    def delta(t_from: float | None, t_to: float | None) -> float | None:
        if t_from is None or t_to is None:
            return None
        return (t_to - t_from) * 1000

    return {
        "asr_done_to_tts_start": delta(t_asr_done, t_tts_start),
        "asr_done_to_first_tts_pcm": delta(t_asr_done, t_first_tts_pcm),
        "asr_done_to_last_pcm": delta(t_asr_done, t_last_pcm),
        "total_ms": (time.perf_counter() - t_start) * 1000,
        "llm_sentence_count": llm_sentence_count,
        "tts_chunk_count": tts_chunk_count,
        "post_tts_pcm_chunks": post_tts_pcm_chunks,
        "error_events": error_events,
    }


def fetch_measure(base_url: str) -> dict:
    """POST /measure and return the aggregated KPI dict (or {"error": ...})."""
    try:
        req = urllib.request.Request(
            f"{base_url}/measure",
            data=b"{}",
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=5.0) as r:
            return json.loads(r.read())
    except Exception as e:
        return {"error": str(e)}


def summary(
    name: str,
    results: list[dict],
    key: str,
    target: float | None = None,
    unit: str = "ms",
) -> None:
    """Print p50/min/max/avg for ``key`` across ``results``."""
    vals = [r[key] for r in results if r.get(key) is not None]
    if not vals:
        print(f"  {name}: no data")
        return
    p50 = sorted(vals)[len(vals) // 2]
    mn, mx = min(vals), max(vals)
    avg = sum(vals) / len(vals)
    tgt = ""
    if target:
        ok = "PASS" if p50 < target else "FAIL"
        tgt = f"  (target <{target}{unit} {ok})"
    print(f"  {name}: p50={p50:.0f}{unit}  min={mn:.0f}  max={mx:.0f}  avg={avg:.0f}{tgt}")


def orchestrator_ws_url(orchestrator_http: str, session_id: str) -> str:
    """Convert ``http://host:port`` into ``ws://host:port/ws?session_id=...``."""
    return orchestrator_http.replace("http://", "ws://") + \
        f"/ws?session_id={session_id}"
