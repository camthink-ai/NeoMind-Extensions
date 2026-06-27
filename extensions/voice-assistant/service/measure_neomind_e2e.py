#!/usr/bin/env python3
"""NeoMind integration end-to-end measurement.

Validates the default profile (``neomind_ws`` LLM backend) end-to-end:

  audio → VAD → ASR → NeoMind chat WS (streaming) → sentence-buffered TTS

This is the Path A acceptance harness. It is profile-agnostic at the wire
level (the orchestrator already wires ``NeoMindWSClient`` per the default
profile), but adds three NeoMind-specific concerns on top of the shared
``measure_common`` WS machinery:

  1. Preflight — warn loudly if ``NEOMIND_TOKEN`` / ``NEOMIND_API_KEY`` is
     unset or the orchestrator is unreachable. Without a token the
     ``neomind_ws`` backend will connect unauthenticated and either get
     rejected or fall back silently.

  2. Reporting — explicitly surface ``llm_ttfb_ms`` and
     ``llm_first_sentence_ms`` from ``/measure``. These are the hard
     evidence that the NeoMind chat WS is the *actual* LLM source: if the
     connection failed and the pipeline bailed out, both KPIs stay empty.

  3. Error classification — a WS ``{"type":"error",...}`` frame is mapped
     to one of three buckets with actionable diagnostics:
       - NeoMind auth failure     (401 / 403 in message)
       - NeoMind unreachable      (connection refused / timed out / DNS)
       - orchestrator-internal     (everything else)

Usage:
    python measure_neomind_e2e.py --n 3
    python measure_neomind_e2e.py --orchestrator http://127.0.0.1:9384 --n 5

Expects the orchestrator already running with ``VOICE_ASSISTANT_PROFILE=default``
(the default profile is configured for ``neomind_ws``).
"""
from __future__ import annotations

import argparse
import asyncio
import os
import socket
import time
from urllib.parse import urlparse

import measure_common as mc

# Substrings in orchestrator error messages that indicate the failure mode
# originated inside the NeoMind chat WS leg rather than the orchestrator.
NEOMIND_AUTH_HINTS = ("401", "403", "unauthorized", "unauthenticated",
                      "forbidden", "api_key", "api key", "invalid token")
NEOMIND_UNREACHABLE_HINTS = ("connection refused", "connection reset",
                             "connection aborted", "timed out", "timeout",
                             "name or service not known", "nodename nor "
                             "servname provided", "temporary failure in "
                             "name resolution", "eof found in the stream",
                             "connect error")


def preflight(orchestrator_http: str) -> bool:
    """Print actionable diagnostics; return False if the run is doomed."""
    ok = True

    token = (os.environ.get("NEOMIND_TOKEN")
             or os.environ.get("NEOMIND_API_KEY", "")).strip()
    if not token:
        print("⚠ NEOMIND_TOKEN / NEOMIND_API_KEY not set — NeoMind chat WS "
              "will connect unauthenticated.")
        print("  Get one: neomind api-key create   then: "
              "export NEOMIND_TOKEN=nmk_...")
        ok = False
    else:
        masked = token[:6] + "…" + ("***" if len(token) > 9 else "")
        print(f"  NEOMIND_TOKEN: {masked}")

    # Orchestrator reachability — cheap TCP probe.
    parsed = urlparse(orchestrator_http)
    host = parsed.hostname
    port = parsed.port or (443 if parsed.scheme == "https" else 80)
    if host:
        try:
            with socket.create_connection((host, port), timeout=2.0):
                pass
            print(f"  orchestrator: reachable at {host}:{port}")
        except OSError as e:
            print(f"✗ orchestrator not reachable at {host}:{port}: {e}")
            print("  Start it: VOICE_ASSISTANT_PROFILE=default "
                  "python service/server.py --port 9384")
            ok = False

    profile = os.environ.get("VOICE_ASSISTANT_PROFILE", "")
    if profile and profile != "default":
        print(f"⚠ VOICE_ASSISTANT_PROFILE={profile} — this harness assumes "
              "the default profile (neomind_ws). Results may not reflect "
              "NeoMind integration.")
    else:
        print("  VOICE_ASSISTANT_PROFILE: default (neomind_ws)")
    return ok


def classify_error(msg: str) -> str:
    """Map an orchestrator error-message string to a diagnostic bucket."""
    low = msg.lower()
    if any(h in low for h in NEOMIND_AUTH_HINTS):
        return ("NeoMind auth rejected (401/403). Check NEOMIND_TOKEN is a "
                "valid, unexpired NeoMind API key.")
    if any(h in low for h in NEOMIND_UNREACHABLE_HINTS):
        return ("NeoMind chat WS unreachable. Verify network, NeoMind host "
                "in the default profile, and that the NeoMind backend is up.")
    return ("Orchestrator-internal error. Inspect the orchestrator log for "
            "the full traceback.")


def report_neomind_kpis(measure: dict) -> None:
    """Print the NeoMind-evidence KPIs from a /measure response dict."""
    if "error" in measure:
        print(f"\n/measure: unavailable ({measure['error']})")
        return

    def show(kpi: str, label: str) -> None:
        v = measure.get(kpi)
        if not v:
            print(f"  {label}: NOT OBSERVED — NeoMind WS likely never "
                  "produced a token. Treat as failure.")
            return
        print(f"  {label}: p50={v.get('p50', 0):.0f}ms  "
              f"p95={v.get('p95', 0):.0f}ms  "
              f"min={v.get('min', 0):.0f}  max={v.get('max', 0):.0f}")

    print("\n/measure NeoMind-evidence KPIs (non-empty ⇒ NeoMind WS is the "
          "actual LLM source):")
    show("llm_ttfb_ms", "llm_ttfb_ms        (NeoMind → first token)")
    show("llm_first_sentence_ms",
         "llm_first_sentence_ms (NeoMind → first complete sentence)")


async def main():
    p = argparse.ArgumentParser()
    p.add_argument("--orchestrator", default="http://127.0.0.1:9384")
    p.add_argument("--audio",
                   default="/Users/shenmingming/CamThink Project/NeoMind-Extensions/extensions/voice-edge-tts/service/assets/default_prompt.wav")
    p.add_argument("--n", type=int, default=3)
    p.add_argument("--session-id", default=f"neomind-e2e-{int(time.time())}")
    p.add_argument("--skip-preflight", action="store_true",
                   help="Skip env/reachability preflight (still runs turns)")
    args = p.parse_args()

    print("=== NeoMind integration E2E — preflight ===")
    if args.skip_preflight:
        print("  (skipped)")
        preflight_ok = True
    else:
        preflight_ok = preflight(args.orchestrator)
    if not preflight_ok:
        print("\nPreflight failed — continuing anyway (use --skip-preflight "
              "to suppress this message). Errors below are expected.\n")

    ws_url = mc.orchestrator_ws_url(args.orchestrator, args.session_id)
    pcm = mc.load_speech(args.audio)
    print(f"\n[neomind e2e] orchestrator={args.orchestrator}")
    print(f"[neomind e2e] audio: {len(pcm)//2} samples "
          f"({len(pcm)//2/mc.SAMPLE_RATE:.1f}s)  ×{args.n}")

    results = []
    classified: list[str] = []
    for i in range(args.n):
        try:
            r = await asyncio.wait_for(
                mc.run_one_turn(ws_url, pcm), timeout=30.0)
        except asyncio.TimeoutError:
            print(f"  #{i+1}: TIMEOUT")
            continue
        results.append(r)
        errs = r.get("error_events") or []
        if errs:
            for e in errs:
                bucket = classify_error(str(e.get("message", "")))
                classified.append(bucket)
        print(f"  #{i+1}: asr→first_tts_pcm={r['asr_done_to_first_tts_pcm']}ms  "
              f"asr→last_pcm={r['asr_done_to_last_pcm']}ms  "
              f"sentences={r['llm_sentence_count']}  "
              f"errors={len(errs)}")

    if classified:
        print("\n=== Error classification ===")
        for c in classified:
            print(f"  • {c}")

    if not results:
        print("\nNo successful turns.")
        report_neomind_kpis(mc.fetch_measure(args.orchestrator))
        return

    print("\n=== NeoMind E2E summary ===")
    mc.summary("asr_done → first REAL TTS PCM", results,
               "asr_done_to_first_tts_pcm", target=600)
    mc.summary("asr_done → last PCM", results, "asr_done_to_last_pcm")
    mc.summary("total turn", results, "total_ms")

    sentences = [r["llm_sentence_count"] for r in results]
    print(f"  llm_sentence_count: min={min(sentences)}  max={max(sentences)}  "
          f"avg={sum(sentences)/len(sentences):.1f}")

    report_neomind_kpis(mc.fetch_measure(args.orchestrator))

    # Hard pass/fail signal: NeoMind KPIs must have observations.
    measure = mc.fetch_measure(args.orchestrator)
    has_llm_obs = bool(measure.get("llm_ttfb_ms")) and \
        bool(measure.get("llm_first_sentence_ms"))
    if has_llm_obs and not classified:
        print("\nPASS: NeoMind chat WS is the live LLM source.")
    else:
        print("\nFAIL: missing NeoMind KPI observations or error events "
              "recorded — see classification above.")


if __name__ == "__main__":
    asyncio.run(main())
