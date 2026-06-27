#!/usr/bin/env python3
"""
Latency measurement for the PoC pipeline.

Standalone script that measures each hop independently WITHOUT needing the
extension or browser. Run with the Python services up:

    # 1. sensevoice-asr service
    cd extensions/sensevoice-asr/service && ./start.sh &

    # 2. moss-tts-nano service
    cd extensions/moss-tts-nano/service && ./start.sh &

    # 3. measure
    python measure_latency.py

Reports:
    - ASR single-utterance latency (for various durations)
    - TTS first-chunk latency (for various reply lengths)
    - End-to-end (ASR → echo → TTS first chunk)
"""
from __future__ import annotations

import argparse
import asyncio
import base64
import io
import json
import os
import sys
import time
import wave
from pathlib import Path

import httpx
import numpy as np

ASR_URL = os.environ.get("SENSEVOICE_ASR_URL", "http://127.0.0.1:9383")
TTS_URL = os.environ.get("MOSS_TTS_URL", "http://127.0.0.1:9382")
TTS_VOICE = os.environ.get("VOICE_ASSISTANT_VOICE", "Junhao")

SAMPLE_RATE = 16000


# ---------------------------------------------------------------------------
# Synthetic audio generators
# ---------------------------------------------------------------------------
def synth_tone(duration_s: float, freq: float = 440.0) -> bytes:
    """Synthesize a sine tone as int16 LE mono PCM WAV bytes."""
    n = int(SAMPLE_RATE * duration_s)
    t = np.linspace(0, duration_s, n, endpoint=False)
    f = 0.3 * np.sin(2 * np.pi * freq * t)
    pcm = (f * 32767).astype("<i2").tobytes()
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SAMPLE_RATE)
        w.writeframes(pcm)
    return buf.getvalue()


def synth_silence(duration_s: float) -> bytes:
    n = int(SAMPLE_RATE * duration_s)
    pcm = np.zeros(n, dtype="<i2").tobytes()
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SAMPLE_RATE)
        w.writeframes(pcm)
    return buf.getvalue()


# ---------------------------------------------------------------------------
# ASR measurement
# ---------------------------------------------------------------------------
async def measure_asr(client: httpx.AsyncClient, audio_wav: bytes, label: str) -> dict:
    b64 = base64.b64encode(audio_wav).decode()
    t0 = time.perf_counter()
    r = await client.post(
        f"{ASR_URL}/asr",
        json={"audio_base64": b64, "language": "auto", "use_itn": True},
        timeout=30.0,
    )
    elapsed_ms = (time.perf_counter() - t0) * 1000.0
    r.raise_for_status()
    obj = r.json()
    dur_s = float(obj.get("duration_seconds", 0.0))
    return {
        "label": label,
        "audio_duration_ms": dur_s * 1000.0,
        "elapsed_ms": elapsed_ms,
        "rtf": obj.get("rtf", elapsed_ms / 1000.0 / dur_s if dur_s > 0 else 0.0),
        "text": obj.get("text", ""),
    }


# ---------------------------------------------------------------------------
# TTS first-chunk measurement
# ---------------------------------------------------------------------------
async def measure_tts(client: httpx.AsyncClient, text: str, label: str) -> dict:
    """Returns first-byte, first-JSON-line, and total TTS latency + chunk count.

    We measure THREE distinct timestamps because moss-tts-nano emits one
    NDJSON line per text chunk (not per audio frame):

      * first_byte_ms  — when the HTTP body started arriving. This is the
        truest "streaming has begun" signal.
      * first_chunk_ms — when the first COMPLETE NDJSON line (containing
        PCM) was parsed. For moss-tts, this equals "all frames for this
        text chunk have been generated and decoded" because each line
        carries the entire chunk's PCM.
      * total_ms       — when the last byte of the body arrived.
    """
    t0 = time.perf_counter()
    first_byte_ms = None
    first_ms = None
    n_chunks = 0
    total_pcm_bytes = 0
    sr = 48000
    ch = 2

    # Buffer to accumulate partial line across byte chunks.
    line_buf = bytearray()

    async with client.stream(
        "POST",
        f"{TTS_URL}/tts/stream",
        json={
            "text": text,
            "voice": TTS_VOICE,
            "sample_mode": "greedy",
            "response_format": "wav",
        },
        timeout=60.0,
    ) as r:
        r.raise_for_status()
        async for raw in r.aiter_bytes():
            if first_byte_ms is None and raw:
                first_byte_ms = (time.perf_counter() - t0) * 1000.0
            # Parse newline-terminated JSON lines out of the byte stream.
            line_buf.extend(raw)
            while True:
                nl = line_buf.find(b"\n")
                if nl < 0:
                    break
                line_bytes = bytes(line_buf[:nl])
                del line_buf[:nl + 1]
                if not line_bytes.strip():
                    continue
                try:
                    obj = json.loads(line_bytes.decode("utf-8"))
                except json.JSONDecodeError:
                    continue
                if "error" in obj:
                    raise RuntimeError(f"tts error: {obj['error']}")
                if "data" not in obj:
                    continue
                if first_ms is None:
                    first_ms = (time.perf_counter() - t0) * 1000.0
                pcm = base64.b64decode(obj["data"])
                sr = int(obj.get("sample_rate", sr))
                ch = int(obj.get("channels", ch))
                n_chunks += 1
                total_pcm_bytes += len(pcm)

    total_ms = (time.perf_counter() - t0) * 1000.0
    audio_duration_ms = (total_pcm_bytes / 2 / ch) / sr * 1000.0 if sr > 0 else 0.0
    return {
        "label": label,
        "text": text,
        "chars": len(text),
        "first_byte_ms": first_byte_ms if first_byte_ms is not None else total_ms,
        "first_chunk_ms": first_ms if first_ms is not None else total_ms,
        "total_ms": total_ms,
        "n_chunks": n_chunks,
        "audio_duration_ms": audio_duration_ms,
        "rtf": total_ms / audio_duration_ms if audio_duration_ms > 0 else 0.0,
    }


# ---------------------------------------------------------------------------
# End-to-end (simulated)
# ---------------------------------------------------------------------------
async def measure_e2e(client: httpx.AsyncClient, audio_wav: bytes, reply: str) -> dict:
    """ASR + TTS (no echo overhead)."""
    t0 = time.perf_counter()
    # ASR
    b64 = base64.b64encode(audio_wav).decode()
    asr_t0 = time.perf_counter()
    r = await client.post(
        f"{ASR_URL}/asr",
        json={"audio_base64": b64, "language": "auto", "use_itn": True},
        timeout=30.0,
    )
    asr_ms = (time.perf_counter() - asr_t0) * 1000.0
    r.raise_for_status()
    asr_obj = r.json()
    # TTS first chunk (byte-level so we get true first-byte time even when
    # moss-tts emits one giant NDJSON line per text chunk).
    tts_t0 = time.perf_counter()
    first_byte_ms = None
    first_ms = None
    line_buf = bytearray()
    async with client.stream(
        "POST",
        f"{TTS_URL}/tts/stream",
        json={
            "text": reply,
            "voice": TTS_VOICE,
            "sample_mode": "greedy",
        },
        timeout=60.0,
    ) as resp:
        resp.raise_for_status()
        async for raw in resp.aiter_bytes():
            if first_byte_ms is None and raw:
                first_byte_ms = (time.perf_counter() - tts_t0) * 1000.0
            line_buf.extend(raw)
            while True:
                nl = line_buf.find(b"\n")
                if nl < 0:
                    break
                line_bytes = bytes(line_buf[:nl])
                del line_buf[:nl + 1]
                if not line_bytes.strip():
                    continue
                try:
                    obj = json.loads(line_bytes.decode("utf-8"))
                except json.JSONDecodeError:
                    continue
                if "data" in obj:
                    if first_ms is None:
                        first_ms = (time.perf_counter() - tts_t0) * 1000.0
                    break  # only need first chunk for E2E
            if first_ms is not None:
                break
    total_to_first_audio = (time.perf_counter() - t0) * 1000.0
    return {
        "asr_ms": asr_ms,
        "tts_first_byte_ms": first_byte_ms if first_byte_ms is not None else -1,
        "tts_first_chunk_ms": first_ms if first_ms is not None else -1,
        "e2e_to_first_audio_ms": total_to_first_audio,
        "transcript": asr_obj.get("text", ""),
    }


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="latency_report.json", help="output JSON report path")
    args = parser.parse_args()

    report = {
        "asr_url": ASR_URL,
        "tts_url": TTS_URL,
        "voice": TTS_VOICE,
        "asr_measurements": [],
        "tts_measurements": [],
        "e2e_measurements": [],
    }

    async with httpx.AsyncClient() as client:
        # ---- Health check ----
        print(f"=== Pinging services ===")
        try:
            r = await client.get(f"{ASR_URL}/health", timeout=5.0)
            print(f"ASR  /health: {r.status_code} {r.text}")
        except Exception as e:
            print(f"ASR  /health: ERROR {e}", file=sys.stderr)
        try:
            r = await client.get(f"{TTS_URL}/health", timeout=5.0)
            print(f"TTS  /health: {r.status_code} {r.text}")
        except Exception as e:
            print(f"TTS  /health: ERROR {e}", file=sys.stderr)

        # ---- ASR measurements (synthetic silence — just to show RTF is bounded) ----
        print(f"\n=== ASR measurements (synthetic silence) ===")
        for dur_s in (1.0, 3.0, 5.0, 10.0):
            wav = synth_silence(dur_s)
            try:
                m = await measure_asr(client, wav, f"silence_{dur_s:.0f}s")
                report["asr_measurements"].append(m)
                print(f"  {m['label']:14s}  audio={m['audio_duration_ms']:6.0f}ms  "
                      f"elapsed={m['elapsed_ms']:6.0f}ms  rtf={m['rtf']:.3f}  "
                      f"text=\"{m['text'][:30]}\"")
            except Exception as e:
                print(f"  silence_{dur_s:.0f}s: ERROR {e}", file=sys.stderr)

        # ---- TTS measurements (various lengths) ----
        print(f"\n=== TTS measurements (greedy, voice={TTS_VOICE}) ===")
        test_replies = [
            ("short", "你好"),
            ("med_zh", "好的，我已经收到你的消息，正在为你处理。"),
            ("long_zh", "你好，欢迎使用 NeoMind 语音助手。今天天气不错，我们一起讨论一下接下来的开发计划吧。"),
            ("short_en", "Hello"),
            ("med_en", "Sure, I have received your message and I'm working on it now."),
        ]
        for label, text in test_replies:
            try:
                m = await measure_tts(client, text, label)
                report["tts_measurements"].append(m)
                print(f"  {label:8s} chars={m['chars']:3d}  "
                      f"first_byte={m['first_byte_ms']:6.0f}ms  "
                      f"first_chunk={m['first_chunk_ms']:6.0f}ms  "
                      f"total={m['total_ms']:6.0f}ms  chunks={m['n_chunks']:3d}  "
                      f"audio_dur={m['audio_duration_ms']:6.0f}ms")
            except Exception as e:
                print(f"  {label}: ERROR {e}", file=sys.stderr)

        # ---- End-to-end (silence audio + canned reply) ----
        print(f"\n=== E2E measurements (silence ASR + canned TTS reply) ===")
        for dur_s, reply in [(3.0, "好的，我明白了。"), (5.0, "收到，正在处理你的请求。")]:
            wav = synth_silence(dur_s)
            try:
                m = await measure_e2e(client, wav, reply)
                report["e2e_measurements"].append({
                    "input_audio_s": dur_s,
                    "reply": reply,
                    **m,
                })
                print(f"  in={dur_s:.0f}s reply='{reply}'  "
                      f"asr={m['asr_ms']:.0f}ms  tts1st={m['tts_first_chunk_ms']:.0f}ms  "
                      f"e2e_to_first_audio={m['e2e_to_first_audio_ms']:.0f}ms")
            except Exception as e:
                print(f"  e2e {dur_s}s: ERROR {e}", file=sys.stderr)

    Path(args.out).write_text(json.dumps(report, indent=2, ensure_ascii=False))
    print(f"\n=== Report saved to {args.out} ===")

    # Print summary go/no-go
    print(f"\n=== GO/NO-GO ===")
    if report["tts_measurements"]:
        first_bytes = [m["first_byte_ms"] for m in report["tts_measurements"]]
        first_chunks = [m["first_chunk_ms"] for m in report["tts_measurements"]]
        max_first = max(first_chunks)
        avg_first = sum(first_chunks) / len(first_chunks)
        print(f"TTS first-byte:   avg={sum(first_bytes)/len(first_bytes):.0f}ms  max={max(first_bytes):.0f}ms")
        print(f"TTS first-chunk:  avg={avg_first:.0f}ms  max={max_first:.0f}ms")
        if max_first < 500:
            print("  ✓ TARGET MET (<500ms): real-time conversation is feasible")
        elif max_first < 1000:
            print("  ~ MARGINAL (500-1000ms): usable but not ideal")
        else:
            print("  ✗ ABOVE TARGET (>1000ms): moss-tts buffers full text-chunk before emit;")
            print("    need per-frame streaming, smaller text chunks, or different TTS backend")
    if report["e2e_measurements"]:
        e2e = [m["e2e_to_first_audio_ms"] for m in report["e2e_measurements"]]
        print(f"E2E to first audio: avg={sum(e2e)/len(e2e):.0f}ms max={max(e2e):.0f}ms")


if __name__ == "__main__":
    asyncio.run(main())
