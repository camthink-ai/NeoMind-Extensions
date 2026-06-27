#!/usr/bin/env python3
"""Voice turn measurement feeding REAL speech audio.

Same protocol as measure_first_sentence.py but feeds a real speech WAV
(default_prompt.wav) instead of a synthetic sine tone, so ASR returns
non-empty text and the full ASR→LLM→TTS pipeline fires.
"""
from __future__ import annotations
import argparse
import asyncio
import base64
import io
import json
import time
import urllib.request
from pathlib import Path

import numpy as np
import soundfile as sf
import websockets

SAMPLE_RATE = 16000


def load_speech(path: str) -> bytes:
    """Load any audio, downmix to mono, resample to 16kHz int16 LE PCM."""
    data, sr = sf.read(path, dtype="float32", always_2d=False)
    if data.ndim > 1:
        data = data.mean(axis=1)
    if sr != SAMPLE_RATE:
        n_out = int(round(len(data) * SAMPLE_RATE / sr))
        idx = np.linspace(0, len(data) - 1, n_out)
        data = np.interp(idx, np.arange(len(data)), data).astype(np.float32)
    pcm = (np.clip(data, -1.0, 1.0) * 32767).astype("<i2")
    return pcm.tobytes()


async def measure(url: str, speech_pcm: bytes) -> dict:
    tl = {"ws_connect": None, "ready": None, "first_pcm_sent": None,
          "asr_start": None, "asr_done": None, "tts_start": None,
          "first_binary": None, "tts_end": None, "stop": None,
          "transcript": "", "tts_end_payload": None,
          "binary_chunk_count": 0, "total_binary_bytes": 0,
          "binary_chunk_ts": []}
    t0 = time.perf_counter()
    async with websockets.connect(url, max_size=None) as ws:
        tl["ws_connect"] = (time.perf_counter() - t0) * 1000
        await ws.send(json.dumps({"type": "start"}))
        async for msg in ws:
            try:
                obj = json.loads(msg)
            except Exception:
                continue
            if obj.get("type") == "ready":
                tl["ready"] = (time.perf_counter() - t0) * 1000
                break
        # Send speech at ~1x realtime, then 1s of silence to trigger VAD end
        frame_size = int(SAMPLE_RATE * 0.1) * 2  # 100ms frames
        silence = b"\x00" * (SAMPLE_RATE * 2 * 2)  # 2s silence
        for pcm in (speech_pcm, silence):
            for off in range(0, len(pcm), frame_size):
                await ws.send(pcm[off:off + frame_size])
                await asyncio.sleep(0.05)
        tl["first_pcm_sent"] = (time.perf_counter() - t0) * 1000
        async for msg in ws:
            now = time.perf_counter()
            if isinstance(msg, (bytes, bytearray)):
                ts = (now - t0) * 1000
                if tl["first_binary"] is None:
                    tl["first_binary"] = ts
                tl["binary_chunk_count"] += 1
                tl["total_binary_bytes"] += len(msg)
                tl["binary_chunk_ts"].append(ts)
                continue
            try:
                obj = json.loads(msg)
            except Exception:
                continue
            t = obj.get("type"); ts = (now - t0) * 1000
            if t == "asr_start": tl["asr_start"] = ts
            elif t == "transcript":
                tl["asr_done"] = ts; tl["transcript"] = obj.get("text", "")
            elif t == "tts_start": tl["tts_start"] = ts
            elif t == "tts_end":
                tl["tts_end"] = ts; tl["tts_end_payload"] = obj.get("metrics", {})
            elif t == "stop":
                tl["stop"] = ts; break
            elif t in ("error", "barge_in", "skip"):
                tl.setdefault("terminal", {})[t] = ts; break
    return tl


def fmt(v): return f"{v:.0f}ms" if v is not None else "—"


def print_report(tl, sm):
    print("\n" + "=" * 64)
    print(f"Transcript: \"{tl.get('transcript', '')}\"")
    print("\nClient-side timeline (ms from ws connect):")
    for k in ("ws_connect","ready","first_pcm_sent","asr_start","asr_done",
             "tts_start","first_binary","tts_end","stop"):
        print(f"  {k:<18} {fmt(tl[k])}")
    print("\nKey user-perceived gaps:")
    pairs = [
        ("ASR done → first audio",    tl["asr_done"],    tl["first_binary"]),
        ("VAD end → ASR done",        tl["first_pcm_sent"], tl["asr_done"]),
        ("ASR done → TTS start",      tl["asr_done"],    tl["tts_start"]),
        ("TTS start → first binary",  tl["tts_start"],   tl["first_binary"]),
        ("Turn total (pcm→stop)",     tl["first_pcm_sent"], tl["stop"]),
    ]
    for label, a, b in pairs:
        if a is not None and b is not None:
            print(f"  {label:<28} {b - a:>6.0f}ms")
    print()
    n = tl["binary_chunk_count"]
    bytes_ = tl["total_binary_bytes"]
    print(f"Binary PCM chunks: {n}  ({bytes_} bytes, "
          f"~{bytes_ / 2 / SAMPLE_RATE * 1000:.0f}ms of audio)")
    if tl.get("tts_end_payload"):
        p = tl["tts_end_payload"]
        print("\nServer-reported metrics (in tts_end frame):")
        for k in ("total_ms","tts_first_chunk_ms","asr_ms"):
            if k in p: print(f"  {k:<22} {p[k]:.0f}ms")
    if sm:
        print("\nServer-side RollingPercentile (/measure):")
        for kpi in ("asr_complete_ms","llm_ttfb_ms","tts_first_chunk_ms",
                    "first_audio_out_ms","full_turn_ms"):
            v = sm.get(kpi)
            if isinstance(v, dict):
                print(f"  {kpi:<22} p50={v.get('p50',0):.0f}ms  "
                      f"p95={v.get('p95',0):.0f}ms")
    print("=" * 64)


async def main():
    p = argparse.ArgumentParser()
    p.add_argument("--url", default="ws://127.0.0.1:9384/ws")
    p.add_argument("--http", default="http://127.0.0.1:9384")
    p.add_argument("--n", type=int, default=2)
    p.add_argument("--audio",
                   default="/Users/shenmingming/CamThink Project/NeoMind-Extensions/extensions/voice-edge-tts/service/assets/default_prompt.wav")
    args = p.parse_args()
    speech = load_speech(args.audio)
    print(f"Loaded {len(speech)} bytes PCM ({len(speech)//2/SAMPLE_RATE:.2f}s) "
          f"from {Path(args.audio).name}")
    for i in range(args.n):
        if i > 0:
            print("\n\n--- next iteration ---")
            await asyncio.sleep(2)
        tl = await measure(args.url, speech)
        sm = None
        try:
            with urllib.request.urlopen(f"{args.http}/measure", timeout=2) as r:
                sm = json.loads(r.read())
        except Exception as e:
            print(f"  (could not fetch /measure: {e})")
        print_report(tl, sm)


if __name__ == "__main__":
    asyncio.run(main())
