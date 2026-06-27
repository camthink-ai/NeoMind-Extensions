#!/usr/bin/env python3
"""Phase 1 standalone latency measurement.

Measures each service directly (no orchestrator):
  - ASR:   POST /asr     → time-to-text (full utterance)
  - TTS:   POST /tts/stream → first-chunk latency + total time
  - Combined simulated turn: ASR_fixed_text → LLM(faked) → TTS first chunk

This isolates the new Kokoro/Qwen3 MLX backends from orchestrator-side VAD issues.
"""
from __future__ import annotations
import argparse, asyncio, base64, io, json, time, wave
import urllib.request

import numpy as np
import soundfile as sf

SAMPLE_RATE = 16000


def load_speech(path: str) -> bytes:
    data, sr = sf.read(path, dtype="float32", always_2d=False)
    if data.ndim > 1: data = data.mean(axis=1)
    if sr != SAMPLE_RATE:
        n_out = int(round(len(data) * SAMPLE_RATE / sr))
        idx = np.linspace(0, len(data) - 1, n_out)
        data = np.interp(idx, np.arange(len(data)), data).astype(np.float32)
    pcm = (np.clip(data, -1, 1) * 32767).astype("<i2")
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SAMPLE_RATE)
        w.writeframes(pcm.tobytes())
    return buf.getvalue()


def http_post_json(url: str, body: dict, timeout: float = 30.0) -> dict:
    req = urllib.request.Request(url,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read())


def measure_asr(url: str, wav_bytes: bytes, n: int) -> list[dict]:
    print(f"\n[ASR] {url}/asr  ×{n}")
    b64 = base64.b64encode(wav_bytes).decode()
    results = []
    for i in range(n):
        t0 = time.perf_counter()
        r = http_post_json(f"{url}/asr",
            {"audio_base64": b64, "language": "auto", "use_itn": True})
        elapsed = (time.perf_counter() - t0) * 1000
        results.append({"elapsed": elapsed, **r})
        print(f"  #{i+1}: {elapsed:.0f}ms  "
              f"(srv {r.get('elapsed_seconds',0)*1000:.0f}ms, "
              f"text=\"{r.get('text','')[:30]}\")")
    return results


async def measure_tts_stream(url: str, text: str, voice: str, n: int) -> list[dict]:
    print(f"\n[TTS stream] {url}/tts/stream  text={text!r}  ×{n}")
    import httpx
    results = []
    for i in range(n):
        t0 = time.perf_counter()
        first_chunk_ms = None
        chunk_count = 0
        total_bytes = 0
        async with httpx.AsyncClient(timeout=60.0) as cli:
            async with cli.stream("POST", f"{url}/tts/stream",
                                  json={"text": text, "voice": voice}) as r:
                r.raise_for_status()
                async for line in r.aiter_lines():
                    if not line.strip(): continue
                    chunk_count += 1
                    if first_chunk_ms is None:
                        first_chunk_ms = (time.perf_counter() - t0) * 1000
                    try:
                        obj = json.loads(line)
                        d = obj.get("data")
                        if d: total_bytes += len(base64.b64decode(d))
                    except Exception: pass
        total = (time.perf_counter() - t0) * 1000
        audio_ms = total_bytes // 2 / 24000 * 1000  # mono int16 @24kHz
        results.append({"first_chunk": first_chunk_ms, "total": total,
                       "chunks": chunk_count, "audio_ms": audio_ms})
        print(f"  #{i+1}: first_chunk={first_chunk_ms:.0f}ms  "
              f"total={total:.0f}ms  chunks={chunk_count}  "
              f"audio={audio_ms:.0f}ms")
    return results


def summary(name: str, results: list[dict], key: str, target: float | None = None):
    vals = [r[key] for r in results if r.get(key) is not None]
    if not vals:
        print(f"  {name}: no data"); return
    p50 = sorted(vals)[len(vals)//2]
    mn, mx = min(vals), max(vals)
    avg = sum(vals) / len(vals)
    tgt = f"  (target <{target}ms {'PASS' if p50 < target else 'FAIL'})" if target else ""
    print(f"  {name}: p50={p50:.0f}ms  min={mn:.0f}  max={mx:.0f}  avg={avg:.0f}{tgt}")


async def measure_bi_stream_turn(asr_url: str, tts_url: str, llm_url: str,
                                  voice: str, wav_bytes: bytes, n: int,
                                  model: str = "qwen3.5:0.8b-mlx") -> list[dict]:
    """Simulate one bi-streaming voice turn against the real backends.

    Mirrors the orchestrator's Phase 2 producer/consumer pipeline:
      ASR (real audio) → LLM stream (sentence-buffered) → per-sentence TTS stream

    Reports the perceived latency from ``asr_done`` to ``first_pcm``, which
    is the user-perceived first-audio delay under bi-streaming. Compare to
    the Phase 1 batched 3-sentence first-chunk (329ms) — bi-streaming
    should land near the single-sentence first-chunk (~130ms) because the
    first TTS stream starts as soon as LLM emits sentence 1, before
    sentences 2 and 3 are even generated.
    """
    import httpx
    from sentence_buffer import SentenceBuffer
    print(f"\n[Bi-stream turn] ASR→LLM({llm_url})→TTS  ×{n}")
    b64 = base64.b64encode(wav_bytes).decode()
    results = []
    for i in range(n):
        t_asr_start = time.perf_counter()
        asr_r = http_post_json(f"{asr_url}/asr",
            {"audio_base64": b64, "language": "auto", "use_itn": True})
        t_asr_done = time.perf_counter()
        transcript = asr_r.get("text", "")

        first_pcm_ms: float | None = None
        sentence_count = 0
        chunk_count = 0
        total_bytes = 0
        buf = SentenceBuffer()
        # Bounded queue mirrors orchestrator's maxsize=4.
        q: "asyncio.Queue[str | None]" = asyncio.Queue(maxsize=4)

        async def tts_consumer():
            nonlocal first_pcm_ms, chunk_count, total_bytes
            while True:
                s = await q.get()
                if s is None:
                    return
                async with httpx.AsyncClient(timeout=60.0) as cli:
                    async with cli.stream("POST", f"{tts_url}/tts/stream",
                                          json={"text": s, "voice": voice}) as r:
                        r.raise_for_status()
                        async for line in r.aiter_lines():
                            if not line.strip():
                                continue
                            chunk_count += 1
                            if first_pcm_ms is None:
                                first_pcm_ms = (time.perf_counter() - t_asr_done) * 1000
                            try:
                                obj = json.loads(line)
                                d = obj.get("data")
                                if d:
                                    total_bytes += len(base64.b64decode(d))
                            except Exception:
                                pass

        consumer_task = asyncio.create_task(tts_consumer())
        # LLM stream → sentence queue (producer).
        async with httpx.AsyncClient(timeout=60.0) as cli:
            async with cli.stream("POST", f"{llm_url}/api/chat",
                                  json={"model": model,
                                        "messages": [{"role": "user",
                                                      "content": transcript}],
                                        "stream": True, "think": False}) as r:
                r.raise_for_status()
                async for line in r.aiter_lines():
                    if not line.strip():
                        continue
                    obj = json.loads(line)
                    if obj.get("done"):
                        break
                    chunk = obj.get("message", {}).get("content", "")
                    if not chunk:
                        continue
                    for sentence in buf.feed(chunk):
                        sentence_count += 1
                        await q.put(sentence)
        tail = buf.flush()
        if tail:
            sentence_count += 1
            await q.put(tail)
        await q.put(None)
        await consumer_task

        total_ms = (time.perf_counter() - t_asr_start) * 1000
        results.append({
            "asr_ms": (t_asr_done - t_asr_start) * 1000,
            "first_pcm_ms": first_pcm_ms,
            "total_ms": total_ms,
            "sentences": sentence_count,
            "chunks": chunk_count,
            "audio_ms": total_bytes // 2 / 24000 * 1000,
        })
        print(f"  #{i+1}: asr={results[-1]['asr_ms']:.0f}ms  "
              f"asr_done→first_pcm={first_pcm_ms:.0f}ms  "
              f"total={total_ms:.0f}ms  sentences={sentence_count}  "
              f"chunks={chunk_count}")
    return results


async def main():
    p = argparse.ArgumentParser()
    p.add_argument("--asr", default="http://127.0.0.1:9383")
    p.add_argument("--tts", default="http://127.0.0.1:9385")
    p.add_argument("--llm", default="http://127.0.0.1:11434")
    p.add_argument("--voice", default="zf_xiaoxiao")
    p.add_argument("--audio",
        default="/Users/shenmingming/CamThink Project/NeoMind-Extensions/extensions/voice-edge-tts/service/assets/default_prompt.wav")
    p.add_argument("--n", type=int, default=3)
    p.add_argument("--bi-stream", action="store_true",
                   help="Run the ASR→LLM→TTS bi-streaming end-to-end measurement")
    p.add_argument("--model", default="qwen3.5:0.8b-mlx",
                   help="Ollama model name (must match ollama list)")
    args = p.parse_args()

    wav_bytes = load_speech(args.audio)
    # Warm-up ASR + TTS once (first-call compile overhead)
    print("[warmup] ASR + TTS first calls...")
    http_post_json(f"{args.asr}/asr",
        {"audio_base64": base64.b64encode(wav_bytes).decode(),
         "language": "auto", "use_itn": True})
    async with __import__("httpx").AsyncClient(timeout=60.0) as cli:
        async with cli.stream("POST", f"{args.tts}/tts/stream",
                              json={"text": "预热", "voice": args.voice}) as r:
            async for _ in r.aiter_lines(): pass
    print("[warmup] done\n")

    asr_r = measure_asr(args.asr, wav_bytes, args.n)
    summary("ASR total", asr_r, "elapsed")

    tts_short = await measure_tts_stream(args.tts,
        "你好，很高兴认识你。", args.voice, args.n)
    summary("TTS first_chunk (short)", tts_short, "first_chunk", target=300)
    summary("TTS total (short)", tts_short, "total")

    tts_long = await measure_tts_stream(args.tts,
        "今天天气真不错，我们一起出去走走吧。你想吃点什么？我可以帮你查一下附近的餐厅。",
        args.voice, args.n)
    summary("TTS first_chunk (3-sentence)", tts_long, "first_chunk", target=300)
    summary("TTS total (3-sentence)", tts_long, "total")

    # Simulated single turn: ASR (we have avg) → LLM skipped → TTS first chunk
    # First-audio-from-end-of-speech estimate.
    asr_p50 = sorted([r["elapsed"] for r in asr_r])[len(asr_r)//2]
    tts_p50 = sorted([r["first_chunk"] for r in tts_short])[len(tts_short)//2]
    # Stage filler hack: if ack played, user perceives ack at ~50-100ms after VAD end,
    # then real TTS answer at ack_dur + (tts_first_chunk - ack_dur if overlapping)
    # Without LLM (echo reply), the gap is just ASR + TTS first_chunk.
    sim_first_audio = asr_p50 + tts_p50
    print(f"\n=== Simulated first-audio latency (ASR + TTS first_chunk, no LLM) ===")
    print(f"  ASR p50: {asr_p50:.0f}ms")
    print(f"  TTS p50: {tts_p50:.0f}ms")
    print(f"  Sum:     {sim_first_audio:.0f}ms  "
          f"(target <600ms {'PASS' if sim_first_audio < 600 else 'FAIL'})")

    if args.bi_stream:
        bi_r = await measure_bi_stream_turn(
            args.asr, args.tts, args.llm, args.voice, wav_bytes, args.n,
            model=args.model)
        summary("Bi-stream asr_done→first_pcm", bi_r, "first_pcm_ms", target=300)
        summary("Bi-stream total turn", bi_r, "total_ms")


if __name__ == "__main__":
    asyncio.run(main())
