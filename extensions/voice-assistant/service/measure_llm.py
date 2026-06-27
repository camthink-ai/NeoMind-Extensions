#!/usr/bin/env python3
"""LLM TTFB + multi-turn + barge-in measurement.

Modes:
  --mode ttfb       Single-shot TTFB per model (Ollama + NeoMind WS)
  --mode multiturn  N consecutive turns through orchestrator WS (if running new server)
  --mode bargein    Mid-response interruption timing
"""
from __future__ import annotations

import argparse, asyncio, base64, io, json, os, sys, time, wave
from typing import Optional

import httpx
import numpy as np

try:
    import websockets
except ImportError:
    websockets = None

SAMPLE_RATE = 16000


# ---------------------------------------------------------------------------
# Audio generators
# ---------------------------------------------------------------------------
def synth_tone(d_ms: float, freq=440.0, amp=0.5) -> bytes:
    n = int(SAMPLE_RATE * d_ms / 1000)
    t = np.arange(n) / SAMPLE_RATE
    return (amp * np.sin(2*np.pi*freq*t) * 32767).astype('<i2').tobytes()


def synth_silence(d_ms: float) -> bytes:
    n = int(SAMPLE_RATE * d_ms / 1000)
    return np.zeros(n, dtype='<i2').tobytes()


# ---------------------------------------------------------------------------
# Ollama direct
# ---------------------------------------------------------------------------
async def ollama_ttfb(client, model, prompt):
    t0 = time.perf_counter()
    ttfb = None
    full = ''
    async with client.stream('POST', 'http://127.0.0.1:11434/api/chat',
        json={'model': model, 'messages':[{'role':'user','content':prompt}], 'stream': True, 'think': False},
        timeout=120.0) as r:
        r.raise_for_status()
        async for line in r.aiter_lines():
            if not line.strip(): continue
            obj = json.loads(line)
            content = obj.get('message', {}).get('content', '')
            if content:
                if ttfb is None:
                    ttfb = (time.perf_counter() - t0) * 1000
                full += content
            if obj.get('done'):
                total = (time.perf_counter() - t0) * 1000
                return ttfb, total, full
    return ttfb, (time.perf_counter() - t0) * 1000, full


# ---------------------------------------------------------------------------
# NeoMind WS (with login)
# ---------------------------------------------------------------------------
async def neomind_login(client, base, user, pwd) -> Optional[str]:
    try:
        r = await client.post(f'{base}/api/auth/login',
            json={'username': user, 'password': pwd}, timeout=10.0)
        if r.status_code != 200:
            print(f'  login failed: {r.status_code} {r.text[:200]}', file=sys.stderr)
            return None
        return r.json().get('token')
    except Exception as e:
        print(f'  login exception: {e}', file=sys.stderr)
        return None


async def neomind_ttfb(token, prompt, base='ws://127.0.0.1:9375'):
    url = f'{base}/api/chat?token={token}'
    t0 = time.perf_counter()
    async with websockets.connect(url, max_size=None) as ws:
        await ws.send(json.dumps({
            'type': 'message',
            'content': prompt,
            'voiceMode': True,
            'sessionId': 'llm-ttfb-bench',
        }))
        ttfb = None
        full = ''
        while True:
            try:
                msg = await asyncio.wait_for(ws.recv(), timeout=90.0)
            except asyncio.TimeoutError:
                break
            if isinstance(msg, bytes): continue
            obj = json.loads(msg)
            et = obj.get('type', '')
            if et == 'Content' and obj.get('text'):
                if ttfb is None:
                    ttfb = (time.perf_counter() - t0) * 1000
                full += obj['text']
            elif et in ('end', 'End'):
                break
        total = (time.perf_counter() - t0) * 1000
        return ttfb, total, full


# ---------------------------------------------------------------------------
# Orchestrator WS multi-turn
# ---------------------------------------------------------------------------
async def orc_connect(url):
    ws = await websockets.connect(url, max_size=None)
    # Server sends `ready` only after we send `start`
    await ws.send(json.dumps({'type': 'start'}))
    # Drain until ready (or skip after timeout)
    deadline = asyncio.get_event_loop().time() + 5.0
    while asyncio.get_event_loop().time() < deadline:
        try:
            msg = await asyncio.wait_for(ws.recv(), timeout=2.0)
        except asyncio.TimeoutError:
            break
        try:
            obj = json.loads(msg)
            if obj.get('type') == 'ready': break
        except: break
    return ws


async def orc_send_turn(ws, speech_ms=1200, silence_ms=600):
    """Send one synthetic speech burst + trailing silence to trigger VAD.
    Returns timeline dict.

    NOTE: assumes `start` already sent in orc_connect."""
    tl = {'first_pcm': None, 'asr_start': None, 'transcript': None,
          'tts_start': None, 'first_binary': None, 'tts_end': None, 'stop': None,
          'transcript_text': '', 'binary_chunks': 0, 'binary_bytes': 0}
    t0 = time.perf_counter()
    # Stream speech in 100ms frames
    sp = synth_tone(speech_ms)
    si = synth_silence(silence_ms)
    fsz = int(SAMPLE_RATE * 0.1) * 2
    for pcm in (sp, si):
        for off in range(0, len(pcm), fsz):
            await ws.send(pcm[off:off+fsz])
            await asyncio.sleep(0.05)
    tl['first_pcm'] = (time.perf_counter() - t0) * 1000

    # Drain frames
    while True:
        try:
            msg = await asyncio.wait_for(ws.recv(), timeout=90.0)
        except asyncio.TimeoutError:
            break
        now = time.perf_counter()
        ts = (now - t0) * 1000
        if isinstance(msg, (bytes, bytearray)):
            if tl['first_binary'] is None: tl['first_binary'] = ts
            tl['binary_chunks'] += 1
            tl['binary_bytes'] += len(msg)
            continue
        try: obj = json.loads(msg)
        except: continue
        t = obj.get('type')
        if t == 'asr_start':   tl['asr_start'] = ts
        elif t == 'transcript':
            tl['transcript'] = ts
            tl['transcript_text'] = obj.get('text', '')
        elif t == 'tts_start': tl['tts_start'] = ts
        elif t == 'tts_end':   tl['tts_end'] = ts
        elif t == 'stop':      tl['stop'] = ts; break
        elif t in ('error', 'barge_in', 'skip'): break
    return tl


async def orc_bargein_turn(ws, speech_ms=1200, silence_ms=600, barge_after_ms=400):
    """Start a turn, then mid-TTS send a `stop` control frame to trigger
    synchronous barge-in cleanup (browser-style interrupt).

    Returns timeline.

    NOTE: assumes `start` already sent in orc_connect."""
    tl = {'first_pcm': None, 'tts_start': None, 'barge_in_sent': None,
          'barge_in_ack': None, 'final_stop': None,
          'binary_before': 0, 'binary_after': 0}
    t0 = time.perf_counter()
    sp = synth_tone(speech_ms)
    si = synth_silence(silence_ms)
    fsz = int(SAMPLE_RATE * 0.1) * 2
    for pcm in (sp, si):
        for off in range(0, len(pcm), fsz):
            await ws.send(pcm[off:off+fsz])
            await asyncio.sleep(0.05)
    tl['first_pcm'] = (time.perf_counter() - t0) * 1000

    barge_sent = False
    while True:
        try:
            msg = await asyncio.wait_for(ws.recv(), timeout=90.0)
        except asyncio.TimeoutError:
            break
        now = time.perf_counter()
        ts = (now - t0) * 1000
        if isinstance(msg, (bytes, bytearray)):
            if not barge_sent: tl['binary_before'] += 1
            else:              tl['binary_after']  += 1
            continue
        try: obj = json.loads(msg)
        except: continue
        t = obj.get('type')
        if t == 'tts_start':
            tl['tts_start'] = ts
            # Wait briefly so we're mid-stream, then send stop control frame.
            # This triggers server-side handle_barge_in() which:
            #   1. transitions FSM → BARGED
            #   2. cancels LLM (NeoMind WS __CANCEL__ or Ollama sets _cancelled flag)
            #   3. clears pending queues
            #   4. notifies browser via barge_in frame
            #   5. transitions BARGED → LISTENING
            await asyncio.sleep(barge_after_ms / 1000)
            await ws.send(json.dumps({'type': 'stop'}))
            tl['barge_in_sent'] = (time.perf_counter() - t0) * 1000
            barge_sent = True
        elif t == 'barge_in':
            tl['barge_in_ack'] = ts
        elif t in ('stop', 'error', 'skip'):
            tl['final_stop'] = ts
            break
    return tl


# ---------------------------------------------------------------------------
# Modes
# ---------------------------------------------------------------------------
async def mode_ttfb(args):
    prompts = [('short_zh', '你好'),
               ('med_zh',   '用一句话介绍你自己'),
               ('long_zh',  '用中文写一首关于春天的诗,4句'),
               ('short_en', 'Hello'),
               ('med_en',   'Explain LLMs in two sentences.')]
    models = args.models.split(',') if args.models else ['qwen3.5:0.8b-mlx', 'qwen3.5:2b-mlx', 'granite4.1:3b', 'nemotron-3-nano:4b']

    print(f'\n=== LLM TTFB (Ollama direct, {len(models)} models × {len(prompts)} prompts) ===')
    async with httpx.AsyncClient() as c:
        print(f'{"model":<22s} {"prompt":<10s} {"TTFB":>8s} {"total":>8s} {"chars":>6s} {"tok/s":>7s}')
        print('-' * 70)
        for m in models:
            # warmup
            try: await ollama_ttfb(c, m, 'hi')
            except Exception as e: print(f'  warmup {m}: {e}', file=sys.stderr)
            for label, p in prompts:
                try:
                    ttfb, total, full = await ollama_ttfb(c, m, p)
                    tps = len(full) / (total/1000) if total > 0 else 0
                    print(f'{m:<22s} {label:<10s} {ttfb:>7.0f}ms {total:>7.0f}ms {len(full):>6d} {tps:>6.1f}')
                except Exception as e:
                    print(f'{m:<22s} {label:<10s} ERROR: {e}', file=sys.stderr)

    # NeoMind WS if creds
    if args.user and args.pwd:
        print(f'\n=== LLM TTFB (NeoMind WS ws://127.0.0.1:9375/api/chat) ===')
        async with httpx.AsyncClient() as c:
            token = await neomind_login(c, 'http://127.0.0.1:9375', args.user, args.pwd)
            if not token:
                print('  login failed; skipping NeoMind WS')
                return
            print(f'  login ok, token len={len(token)}')
            print(f'{"prompt":<10s} {"TTFB":>8s} {"total":>8s} {"chars":>6s}')
            for label, p in prompts:
                try:
                    ttfb, total, full = await neomind_ttfb(token, p)
                    print(f'{label:<10s} {ttfb:>7.0f}ms {total:>7.0f}ms {len(full):>6d}  "{full[:40]}"')
                except Exception as e:
                    print(f'{label:<10s} ERROR: {e}', file=sys.stderr)


async def mode_multiturn(args):
    if websockets is None:
        print('websockets package required'); return
    print(f'\n=== Multi-turn via orchestrator ({args.url}, n={args.n}) ===')
    print('NOTE: requires the NEW FastAPI orchestrator (post-fix).')
    for i in range(args.n):
        try:
            ws = await orc_connect(args.url)
        except Exception as e:
            print(f'  turn {i+1}: connect failed: {e}'); return
        t0 = time.perf_counter()
        tl = await orc_send_turn(ws)
        await ws.close()
        # Report
        gaps = []
        if tl['transcript'] and tl['first_binary']:
            gaps.append(('ASR→first_audio', tl['first_binary'] - tl['transcript']))
        if tl['first_pcm'] and tl['stop']:
            gaps.append(('turn_total', tl['stop'] - tl['first_pcm']))
        gap_str = '  '.join(f'{k}={v:.0f}ms' for k, v in gaps)
        print(f'  turn {i+1}: transcript="{tl["transcript_text"][:30]}"  '
              f'chunks={tl["binary_chunks"]}  {gap_str}')


async def mode_bargein(args):
    if websockets is None:
        print('websockets package required'); return
    print(f'\n=== Barge-in test via orchestrator ({args.url}) ===')
    print('NOTE: requires the NEW FastAPI orchestrator (post-fix).')
    try:
        ws = await orc_connect(args.url)
    except Exception as e:
        print(f'  connect failed: {e}'); return
    tl = await orc_bargein_turn(ws, barge_after_ms=args.barge_after)
    await ws.close()
    print(f'  tts_start:        {tl["tts_start"]}')
    print(f'  barge_in sent:    {tl["barge_in_sent"]}')
    print(f'  barge_in ack:     {tl["barge_in_ack"]}')
    print(f'  final stop:       {tl["final_stop"]}')
    print(f'  binary chunks before barge: {tl["binary_before"]}')
    print(f'  binary chunks after barge:  {tl["binary_after"]}')
    if tl['barge_in_sent'] and tl['barge_in_ack']:
        cancel_ms = tl['barge_in_ack'] - tl['barge_in_sent']
        print(f'  >>> Cancel latency (sent→ack): {cancel_ms:.0f}ms')


async def main():
    p = argparse.ArgumentParser()
    p.add_argument('--mode', choices=['ttfb', 'multiturn', 'bargein'], required=True)
    p.add_argument('--url', default='ws://127.0.0.1:9384/ws')
    p.add_argument('--n', type=int, default=5)
    p.add_argument('--models', default='', help='comma-sep ollama models')
    p.add_argument('--user', default=os.environ.get('NEOMIND_USER', ''))
    p.add_argument('--pwd',  default=os.environ.get('NEOMIND_PWD', ''))
    p.add_argument('--barge-after', type=int, default=400,
                   help='ms after tts_start to fire barge-in')
    args = p.parse_args()
    if args.mode == 'ttfb':      await mode_ttfb(args)
    elif args.mode == 'multiturn': await mode_multiturn(args)
    elif args.mode == 'bargein': await mode_bargein(args)


if __name__ == '__main__':
    asyncio.run(main())
