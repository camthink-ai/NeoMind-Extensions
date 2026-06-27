#!/usr/bin/env python3
"""
Echo-rejection measurement (Opt 3 — AEC echo window).

Validates that the voice-assistant orchestrator suppresses TTS-leak
phantom transcripts. Approach:

    1. Connect to orchestrator WS.
    2. Send real speech PCM (1s loud tone) → triggers ASR → TTS playback.
    3. The moment first TTS binary PCM arrives, start sending "leak"
       PCM (lower-amplitude tone emulating speaker→mic leakage).
    4. Stop leak after ``leak_duration_s``.
    5. Count phantom transcripts (``type=transcript`` events received
       while TTS is playing or within ``tail_ms`` after).

Target:
    AEC off (VOICE_ASSISTANT_AEC_MODE=off):  phantom rate ≈ 80%
    AEC on  (VOICE_ASSISTANT_AEC_MODE=echo_window): phantom rate < 10%

Run:
    # Server (run twice, once with each AEC mode):
    VOICE_ASSISTANT_AEC_MODE=off python server.py &
    python measure_echo_rejection.py

    VOICE_ASSISTANT_AEC_MODE=echo_window python server.py &
    python measure_echo_rejection.py

Env:
    LEAK_AMPLITUDE      RMS amplitude of fake leak tone (default 0.03,
                        ~slightly above VAD_ENERGY_THRESHOLD of 0.015)
    LEAK_FREQ           tone frequency in Hz (default 220, deep male-like)
    LEAK_DURATION_S     total seconds of leak audio (default 3.0)
"""
from __future__ import annotations

import argparse
import asyncio
import json
import time
from typing import Optional

import numpy as np
import websockets

SAMPLE_RATE = 16000


def synth_tone(duration_s: float, freq: float = 440.0, amplitude: float = 0.3) -> bytes:
    """Sine tone as int16 LE PCM at 16kHz mono."""
    n = int(SAMPLE_RATE * duration_s)
    t = np.linspace(0, duration_s, n, endpoint=False)
    f = amplitude * np.sin(2 * np.pi * freq * t)
    return (f * 32767).astype("<i2").tobytes()


def synth_silence(duration_s: float) -> bytes:
    n = int(SAMPLE_RATE * duration_s)
    return np.zeros(n, dtype="<i2").tobytes()


async def measure_once(
    url: str,
    leak_amplitude: float = 0.03,
    leak_freq: float = 220.0,
    leak_duration_s: float = 3.0,
) -> dict:
    """Run one echo-rejection trial. Returns a result dict."""
    result = {
        "trigger_transcript": None,
        "phantom_transcripts": [],   # list of {t_ms, text}
        "tts_start_ts": None,
        "tts_end_ts": None,
        "first_binary_ts": None,
        "leak_started_ts": None,
        "leak_stopped_ts": None,
        "phantom_count": 0,
    }

    t0 = time.perf_counter()
    async with websockets.connect(url, max_size=None) as ws:
        await ws.send(json.dumps({
            "type": "start",
            "session_id": "measure-echo",
            "sample_rate": SAMPLE_RATE,
            "channels": 1,
            "format": "pcm_int16_le",
        }))

        # Drain until ready
        ready = False
        async for msg in ws:
            try:
                obj = json.loads(msg)
            except Exception:
                continue
            if obj.get("type") == "ready":
                ready = True
                break
        if not ready:
            result["error"] = "no ready from server"
            return result

        # 1) Send loud speech to trigger ASR + TTS pipeline
        speech = synth_tone(1.0, freq=440.0, amplitude=0.3)
        silence = synth_silence(0.8)
        frame_size = int(SAMPLE_RATE * 0.1) * 2

        # Reader task — runs concurrently with sender; logs phantom transcripts
        async def reader():
            while True:
                try:
                    msg = await ws.recv()
                except websockets.ConnectionClosed:
                    return
                now = time.perf_counter()
                if isinstance(msg, (bytes, bytearray)):
                    if result["first_binary_ts"] is None:
                        result["first_binary_ts"] = (now - t0) * 1000
                    continue
                try:
                    obj = json.loads(msg)
                except Exception:
                    continue
                t = obj.get("type")
                ts_ms = (now - t0) * 1000
                if t == "transcript":
                    # First transcript = user speech (the trigger). Subsequent
                    # ones while TTS is active = phantoms from TTS leak.
                    if result["trigger_transcript"] is None:
                        result["trigger_transcript"] = {
                            "t_ms": ts_ms,
                            "text": obj.get("text", ""),
                        }
                    elif result["tts_end_ts"] is None:
                        # TTS still active → phantom
                        result["phantom_transcripts"].append({
                            "t_ms": ts_ms,
                            "text": obj.get("text", ""),
                        })
                elif t == "tts_start":
                    result["tts_start_ts"] = ts_ms
                elif t == "tts_end":
                    result["tts_end_ts"] = ts_ms
                elif t in ("stop", "error", "barge_in"):
                    result.setdefault("terminal", {})[t] = ts_ms
                    if t in ("error",):
                        return

        reader_task = asyncio.create_task(reader())

        # Send trigger speech + silence
        for PCM in (speech, silence):
            for off in range(0, len(PCM), frame_size):
                await ws.send(PCM[off:off + frame_size])
                await asyncio.sleep(0.05)

        # Wait for TTS to actually start (first binary chunk)
        while result["first_binary_ts"] is None:
            await asyncio.sleep(0.02)
            if result.get("terminal"):
                break
        result["leak_started_ts"] = (time.perf_counter() - t0) * 1000

        # 2) Send "leak" tone at lower amplitude (speaker → mic bleed)
        # Continue for leak_duration_s, then stop and wait for tail.
        leak_pcm = synth_tone(leak_duration_s, freq=leak_freq, amplitude=leak_amplitude)
        for off in range(0, len(leak_pcm), frame_size):
            await ws.send(leak_pcm[off:off + frame_size])
            await asyncio.sleep(0.05)
        result["leak_stopped_ts"] = (time.perf_counter() - t0) * 1000

        # Wait for tts_end (or timeout 10s after leak stopped)
        deadline = 10.0
        waited = 0.0
        while result["tts_end_ts"] is None and waited < deadline:
            await asyncio.sleep(0.1)
            waited += 0.1

        # Give a short grace period for any in-flight transcripts to land
        await asyncio.sleep(0.5)
        reader_task.cancel()
        try:
            await reader_task
        except (asyncio.CancelledError, Exception):
            pass

    result["phantom_count"] = len(result["phantom_transcripts"])
    return result


def print_report(r: dict, trial: int) -> None:
    print()
    print("=" * 64)
    print(f"Trial {trial}")
    if r.get("error"):
        print(f"  ERROR: {r['error']}")
        return
    trig = r.get("trigger_transcript") or {}
    print(f"  Trigger transcript:  \"{trig.get('text', '')}\"  @ {trig.get('t_ms', -1):.0f}ms")
    print(f"  TTS started:         {fmt(r['tts_start_ts'])}")
    print(f"  First binary chunk:  {fmt(r['first_binary_ts'])}")
    print(f"  Leak started:        {fmt(r['leak_started_ts'])}")
    print(f"  Leak stopped:        {fmt(r['leak_stopped_ts'])}")
    print(f"  TTS ended:           {fmt(r['tts_end_ts'])}")
    print()
    n = r["phantom_count"]
    print(f"  Phantom transcripts during TTS: {n}")
    for p in r["phantom_transcripts"][:5]:
        snippet = p["text"][:40] + ("…" if len(p["text"]) > 40 else "")
        print(f"    @ {p['t_ms']:.0f}ms  \"{snippet}\"")
    if n > 5:
        print(f"    ... {n - 5} more")
    verdict = "PASS (<1 phantom)" if n < 1 else (
        "PASS (<10% phantom rate, single trial)" if n <= 1 else "FAIL"
    )
    print(f"  Verdict (single trial): {verdict}")
    print("=" * 64)


def fmt(v):
    return f"{v:.0f}ms" if v is not None else "—"


async def main():
    p = argparse.ArgumentParser()
    p.add_argument("--url", default="ws://127.0.0.1:9384/ws")
    p.add_argument("--n", type=int, default=3, help="iterations")
    p.add_argument("--leak-amp", type=float, default=float(__import__("os").environ.get("LEAK_AMPLITUDE", "0.03")))
    p.add_argument("--leak-freq", type=float, default=float(__import__("os").environ.get("LEAK_FREQ", "220")))
    p.add_argument("--leak-duration", type=float, default=float(__import__("os").environ.get("LEAK_DURATION_S", "3.0")))
    args = p.parse_args()

    total_phantoms = 0
    for i in range(args.n):
        if i > 0:
            print("\n\n--- next trial ---")
            await asyncio.sleep(2)
        r = await measure_once(
            args.url,
            leak_amplitude=args.leak_amp,
            leak_freq=args.leak_freq,
            leak_duration_s=args.leak_duration,
        )
        print_report(r, i + 1)
        total_phantoms += r["phantom_count"]

    print(f"\nSummary: {total_phantoms} phantom transcripts over {args.n} trials "
          f"(avg {total_phantoms / args.n:.1f}/trial)")
    print("Target: AEC on → <0.1/trial   |   AEC off → ~1+/trial")


if __name__ == "__main__":
    asyncio.run(main())
