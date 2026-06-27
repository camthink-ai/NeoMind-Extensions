#!/usr/bin/env python3
"""Phase 2 bi-streaming end-to-end latency measurement via the WS orchestrator.

Drives the real VoicePipeline (kokoro-qwen3 profile by default) with a real
audio prompt, then reports user-perceived latencies from the WS frames the
orchestrator emits:

  asr_done → first_pcm    : perceived first-audio delay (target < 600ms)
  asr_done → last_pcm     : full turn duration
  llm_sentence_count      : sentences emitted (proves bi-streaming)
  tts_chunk_count         : PCM binary frames received
  llm_first_sentence_ms   : pulled from /measure (LLM → first complete sentence)

Usage:
    python measure_bi_stream_e2e.py --n 5
    python measure_bi_stream_e2e.py --profile kokoro-qwen3 --orchestrator http://127.0.0.1:9384

Expects the orchestrator already running with VOICE_ASSISTANT_PROFILE=kokoro-qwen3.

WS protocol handling, audio loading, and /measure fetch live in
``measure_common`` and are shared with ``measure_neomind_e2e.py``.
"""
from __future__ import annotations

import argparse
import asyncio
import time

import measure_common as mc


async def main():
    p = argparse.ArgumentParser()
    p.add_argument("--orchestrator", default="http://127.0.0.1:9384")
    p.add_argument("--profile", default="kokoro-qwen3",
                   help="Profile name (informational — orchestrator must already run it)")
    p.add_argument("--audio",
                   default="/Users/shenmingming/CamThink Project/NeoMind-Extensions/extensions/voice-edge-tts/service/assets/default_prompt.wav")
    p.add_argument("--n", type=int, default=3)
    p.add_argument("--session-id", default=f"bi-stream-e2e-{int(time.time())}")
    args = p.parse_args()

    ws_url = mc.orchestrator_ws_url(args.orchestrator, args.session_id)
    pcm = mc.load_speech(args.audio)
    print(f"[bi-stream e2e] orchestrator={args.orchestrator}  profile={args.profile}")
    print(f"[bi-stream e2e] audio: {len(pcm)//2} samples ({len(pcm)//2/mc.SAMPLE_RATE:.1f}s)  ×{args.n}")

    results = []
    for i in range(args.n):
        try:
            r = await asyncio.wait_for(
                mc.run_one_turn(ws_url, pcm), timeout=30.0)
        except asyncio.TimeoutError:
            print(f"  #{i+1}: TIMEOUT")
            continue
        results.append(r)
        print(f"  #{i+1}: asr→tts_start={r['asr_done_to_tts_start']}ms  "
              f"asr→first_tts_pcm={r['asr_done_to_first_tts_pcm']}ms  "
              f"asr→last_pcm={r['asr_done_to_last_pcm']}ms  "
              f"sentences={r['llm_sentence_count']}  "
              f"post_tts_chunks={r['post_tts_pcm_chunks']}")

    if not results:
        print("\nNo successful turns.")
        return

    print("\n=== Bi-streaming E2E summary ===")
    mc.summary("asr_done → tts_start (orchestrator)", results,
               "asr_done_to_tts_start")
    mc.summary("asr_done → first REAL TTS PCM", results,
               "asr_done_to_first_tts_pcm", target=600)
    mc.summary("asr_done → last PCM", results, "asr_done_to_last_pcm")
    mc.summary("total turn", results, "total_ms")

    sentences = [r["llm_sentence_count"] for r in results]
    chunks = [r["tts_chunk_count"] for r in results]
    print(f"  llm_sentence_count: min={min(sentences)}  max={max(sentences)}  "
          f"avg={sum(sentences)/len(sentences):.1f}")
    print(f"  tts_chunk_count:    min={min(chunks)}  max={max(chunks)}  "
          f"avg={sum(chunks)/len(chunks):.1f}")

    measure = mc.fetch_measure(args.orchestrator)
    if "error" in measure:
        print(f"\n/measure: unavailable ({measure['error']})")
    else:
        llm_fs = measure.get("llm_first_sentence_ms", {})
        if llm_fs:
            print(f"\n/measure: llm_first_sentence_ms p50={llm_fs.get('p50', 0):.0f}ms  "
                  f"p95={llm_fs.get('p95', 0):.0f}ms")
        else:
            print("\n/measure: llm_first_sentence_ms not yet observed")


if __name__ == "__main__":
    asyncio.run(main())
