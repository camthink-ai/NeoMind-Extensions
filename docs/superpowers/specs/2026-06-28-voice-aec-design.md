# Voice Assistant Server-Side AEC (Reference-Based)

**Date:** 2026-06-28
**Status:** Design — pending implementation plan
**Scope:** `extensions/voice-assistant/` (Python orchestrator + companion measurement script)

## Motivation

The voice-assistant pipeline has three sources of server-pushed PCM —
turn TTS, greeting, ack, and stage-filler — that all play through the
browser speaker and leak back into the mic. The current "AEC" is a
half-duplex VAD-threshold hack (`AEC_MODE=echo_window`,
`server.py:115-119, 412-419`): while TTS is playing and within a 400ms
tail window, VAD threshold is boosted (energy VAD +0.020 RMS) or
required silence is extended (FSMN VAD +800ms). The
`measure_echo_rejection.py` harness reports this cuts phantom
transcript rate from ~80% (AEC off) to <10% (echo_window), but the
mechanism is **deafening**, not **cancelling**:

- User must wait for TTS playback to end, or speak louder than the
  speaker output to trigger barge-in (not full-duplex).
- Greeting, ack, and stage-filler are all opt-in / disabled by default
  precisely because the echo_window mitigation is insufficient for
  longer clips. See `docs/superpowers/specs/2026-06-27-voice-greeting-design.md`
  "Known limitation" section.
- `contracts.py:91` declares an `AECBackend` Protocol with
  `process(mic_pcm, reference_pcm) -> bytes`, and `Profile.aec_config`
  exists in `profile.py:19` — both unused. The seam is open; the body
  is missing.

Production voice AI stacks (LiveKit Agents, Vapi, OpenAI Realtime)
all ship **server-side reference-based AEC**: the server knows what
PCM it just pushed to the browser, keeps it as a reference signal,
and subtracts it from the upstream mic PCM via an adaptive filter
(WebRTC AEC3 typically). The browser is treated as a dumb player.
This spec brings voice-assistant to that pattern.

## Goals

1. Plug the `AECBackend` Protocol with concrete adapters — at minimum
   `SherpaAECBackend` (wraps `sherpa-onnx`, already a project
   dependency) — so mic PCM is genuinely de-echoed before VAD/ASR.
2. Maintain a single `ReferenceRingBuffer` that captures **all**
   server-pushed PCM (TTS + greeting + ack + stage-filler) as the
   reference signal, with a profile-tunable static delay.
3. Stay opt-in: `default.yaml` keeps `acoustic.aec: echo_window`.
   Users who want full-duplex barge-in flip one field to `sherpa` (or
   `webrtc` after measurement).
4. Auto-fallback: if the configured AEC library fails to import or
   initialize, log a warning and downgrade to `NoopAECBackend`
   (equivalent to AEC off), letting the existing `echo_window` logic
   still apply via `_aec_active_now()`.
5. Measurement-driven library selection: extend
   `measure_echo_rejection.py` with a `--backend` flag so the
   maintainer can run sherpa vs webrtc vs echo_window head-to-head
   and pick the default based on ERLE (Echo Return Loss Enhancement)
   data, not guesswork.
6. Zero protocol change: the WS protocol is unchanged. The reference
   signal is harvested server-side from existing `send_binary` calls.

## Non-Goals

- **Browser-side reference loopback.** Architecture B (browser sends
  actual playback back as upstream reference) was considered and
  deferred. It requires WS protocol changes, poc.html JS rework,
  Rust cdylib proxy changes, and doubles upstream bandwidth. Static
  delay calibration is good enough for LiveKit and is good enough
  here. Deferred until measurement shows static delay inadequate.
- **Browser-side AEC diagnostics.** Whether `getUserMedia`
  `echoCancellation:true` is or isn't catching Web Audio API output
  is a separate investigation. This spec treats the server as the
  canonical AEC location regardless.
- **Custom NLMS / hand-rolled adaptive filter.** YAGNI; WebRTC AEC3
  via a library is the proven path.
- **Continuous AEC tuning / room adaptation measurements.** The spec
  ships one delay value per profile; auto-calibration is future work.
- **Multi-channel / stereo AEC.** Voice-assistant is mono 16kHz only.
- **Per-session reference buffer.** Reference buffer is global
  (server-wide) because all sessions share the same TTS/ack/greeting
  PCM content; per-session duplication is wasteful. (See "Open
  Questions" — if double-talk across simultaneous sessions is ever
  observed this can be revisited.)

## Architecture

### Data flow

```
Browser mic PCM (16kHz mono int16 LE)
      │
      ▼
 VoiceSession.feed_pcm(samples)         ← existing entry point
      │
      ▼
 _preprocess_with_aec(samples)         ← NEW: AEC + ring buffer peek
      │   ref = ref_ring_buffer.peek_window(
      │           delay_ms=AEC_REFERENCE_DELAY_MS,
      │           length_ms=len(samples)/SAMPLE_RATE*1000,
      │       )
      │   cleaned = aec_backend.process_capture(samples, ref)
      │   return cleaned
      ▼
 feed_pcm_internals(cleaned_samples)   ← existing VAD logic, untouched
      │
      ▼
 (VAD → ASR → LLM → TTS, unchanged)
      │
      ▼
 sess.send_binary(pcm_bytes)           ← existing, unchanged signature
      │
      └──► ref_ring_buffer.push(pcm_bytes)   ← NEW: side effect, all
                                                send_binary call sites
                                                also push to ref buffer
```

The AEC step is a pure preprocessing transform. All downstream
consumers (VAD, ASR, barge-in guard) see cleaned PCM and require no
changes.

### Component responsibilities

| Component | File | Responsibility |
|-----------|------|----------------|
| `AECBackend` Protocol | `contracts.py` | Refine to `init(sr) → bool`, `process_capture(mic_int16, ref_int16) → cleaned_int16`, `close()`. |
| `ReferenceRingBuffer` | `aec.py` (new) | Fixed-capacity int16 ring of last N seconds; `push(int16_bytes)`, `peek_window(delay_ms, length_ms) → int16_bytes` zero-padded if window underflows. |
| `SherpaAECBackend` | `backends/aec.py` (new) | Wraps `sherpa.onnx.Aec` (or whatever sherpa-onnx exposes); implements `AECBackend`. |
| `WebRtcAECBackend` | `backends/aec.py` (new, deferred) | Wraps `webrtc_audio_processing` library. Imported lazily; only triggered if profile says `type: webrtc`. |
| `NoopAECBackend` | `backends/aec.py` (new) | `process_capture` returns mic unchanged. Used when `type: none` or as fallback on init failure. |
| `make_aec(profile)` | `backends/__init__.py` | Factory: reads `profile.aec_config["type"]`, returns adapter; on import/init failure logs warning and returns `NoopAECBackend`. |
| `_ref_ring_buffer` | `server.py` (module global) | Single `ReferenceRingBuffer` instance, capacity = `AEC_REF_BUFFER_SECONDS * SAMPLE_RATE * 2` bytes. Initialized at startup. |
| `_aec_backend` | `server.py` (module global) | Single `AECBackend` instance. Initialized at startup via `make_aec(_profile)`. |
| `VoiceSession.feed_pcm` | `server.py` | Add 3-line `_preprocess_with_aec` prologue at top of existing method. |
| `VoiceSession.send_binary` | `server.py` | Add 1-line `push_to_ref_buffer` side effect after the actual `ws.send_bytes` succeeds. |

### Server startup changes

```python
# At module top (after _tts_backend etc.):
_aec_backend: AECBackend = NoopAECBackend()
_ref_ring_buffer: ReferenceRingBuffer  # initialized in lifespan

# In _warm_banks_async lifespan hook (existing):
async def _warm_banks_async() -> None:
    ...
    # NEW: init AEC backend + ref buffer
    global _aec_backend, _ref_ring_buffer
    _aec_backend = make_aec(_profile)  # returns Noop on failure
    _aec_ring_capacity_bytes = int(
        AEC_REF_BUFFER_SECONDS * SAMPLE_RATE * 2  # int16 = 2 bytes
    )
    _ref_ring_buffer = ReferenceRingBuffer(_aec_ring_capacity_bytes)
    await _aec_backend.init(SAMPLE_RATE)
    logger.info("AEC backend ready: %s, ref buffer %.1fs",
                type(_aec_backend).__name__, AEC_REF_BUFFER_SECONDS)
    # existing _warm_ack_bank / _warm_stage_fillers / _warm_greeting
```

### `send_binary` instrumentation

Every site that calls `sess.send_binary(pcm_bytes)` is updated to also
push to the ref buffer. There are 4 call sites in `server.py`:

1. Greeting push (`ws_handler` start branch)
2. Ack playback (`_play_ack_to_session` or similar)
3. Stage filler playback (`_play_stage_filler` or similar)
4. Turn TTS PCM (`on_tts_pcm` callback)

The instrumentation is one line per site:

```python
await sess.send_binary(pcm)
_ref_ring_buffer.push(pcm)  # feed AEC reference path
```

A centralized helper `sess.send_binary_and_ref(pcm)` is a YAGNI
candidate; 4 call sites with a 1-liner is fine. (See "Open Questions".)

### `feed_pcm` AEC preprocessing

Inside `VoiceSession.feed_pcm` (the existing top-level entry),
insert a preprocessing step before any VAD branch:

```python
def feed_pcm(self, samples_int16: np.ndarray) -> Optional[np.ndarray]:
    # NEW: AEC preprocessing (no-op if backend is Noop)
    if _aec_backend is not None and not isinstance(_aec_backend, NoopAECBackend):
        ref = _ref_ring_buffer.peek_window(
            delay_ms=AEC_REFERENCE_DELAY_MS,
            length_ms=len(samples_int16) / SAMPLE_RATE * 1000,
        )
        samples_int16 = _aec_backend.process_capture(samples_int16, ref)
    # existing dispatch (silero / fsmn / energy)
    if self._silero_vad is not None: ...
```

The `isinstance(..., NoopAECBackend)` shortcut avoids the ring-buffer
peek on every frame when AEC is disabled — micro-optimization, but
the ring buffer is shared and peek allocates per call.

### Interaction with existing `echo_window`

`echo_window` (the half-duplex VAD threshold boost) and real AEC are
**complementary, not exclusive**:

- If `acoustic.aec: echo_window` (default): AEC backend is `NoopAECBackend`,
  `echo_window` runs unchanged.
- If `acoustic.aec: sherpa` (opt-in full-duplex): AEC backend is
  `SherpaAECBackend`, AND `echo_window` may still apply as a second
  line of defense (configurable via `acoustic.aec_keep_echo_window:
  false` to fully disable for testing).

The `_aec_active_now()` function's semantics do NOT change — it still
reflects "TTS is currently playing". With real AEC, the echo_window
boost becomes a backup, not the primary mechanism.

### Profile schema changes

`profile.py` `aec_config` shape evolves from `{type: <str>}` to:

```python
{
    "type": "sherpa" | "webrtc" | "echo_window" | "none",
    "reference_delay_ms": 200,   # static calibration
    "ref_buffer_seconds": 3.0,   # ring buffer capacity
    "keep_echo_window": True,    # whether to also apply echo_window boost
}
```

`from_dict` populates these with sensible defaults if missing.
`echo_window` and `none` both result in `NoopAECBackend`, but
`echo_window` keeps `keep_echo_window=True` implicit.

### WS protocol

**Unchanged.** No new frame types, no new fields. The reference path
is entirely server-internal.

## Failure modes

| Failure | Behavior |
|---------|----------|
| AEC library import fails (sherpa-onnx missing AEC submodule) | `make_aec` catches `ImportError`, logs `WARNING AEC backend 'sherpa' unavailable: <e>; falling back to Noop`, returns `NoopAECBackend`. Server starts normally with echo_window. |
| `AECBackend.init` returns False or raises | Same fallback. |
| `process_capture` raises during a turn | Caught in `feed_pcm`, logs WARNING once per session, sets `_aec_backend = NoopAECBackend` for the rest of the session (prevents log flood). The current frame passes through uncleaned; subsequent frames skip AEC. |
| Reference ring buffer underflows (delay > buffer capacity) | `peek_window` zero-pads the missing portion. AEC gets partial reference, performs worse but does not crash. Logged once at WARNING when first observed. |
| Profile says `type: webrtc` but library not installed | Same as import failure fallback. |

The fallback contract is: **AEC never breaks the call.** Worst case,
the pipeline degrades to today's `echo_window` behavior.

## Measurement & decision criteria

`measure_echo_rejection.py` is extended with a `--backend` CLI flag
and richer output:

```
python measure_echo_rejection.py --backend sherpa --trials 20
```

Output per run:
- **Phantom rate**: fraction of trials where ≥1 phantom transcript
  arrived during TTS playback + tail window. Existing metric.
- **ERLE (Echo Return Loss Enhancement)**: `10 * log10(sum(mic²) / sum(cleaned²))`
  in dB, computed per frame during known-TTS-playback windows.
  Industry standard AEC quality metric. >15 dB = good, >20 dB = great,
  >25 dB = transparent.
- **Double-talk detection**: count of legitimate user-utterance
  transcripts successfully captured DURING TTS playback (should be
  high with real AEC, ~0 with echo_window alone). This is the
  full-duplex metric that distinguishes real AEC from the hack.

### Decision matrix for default flip

After implementing all three backends (sherpa, webrtc, echo_window),
run `measure_echo_rejection.py --backend <each> --trials 30`. Flip
`default.yaml` to `acoustic.aec: <winner>` only if:

1. Phantom rate with the candidate backend ≤ phantom rate with
   `echo_window` (i.e., real AEC is at least as good at the easy
   metric), AND
2. Double-talk detection ≥ 80% (the metric `echo_window`
   structurally cannot improve), AND
3. ERLE ≥ 15 dB (basic competence threshold).

If no backend clears all three, the default stays `echo_window` and
real AEC remains opt-in. The spec does not promise a default flip —
only the measurement infrastructure to enable a data-driven one.

## Testing

### Unit (`tests/test_aec.py`, new)

- `ReferenceRingBuffer`
  - `push` then `peek_window(0, full)` returns the pushed bytes
  - `peek_window(delay_ms=200, length_ms=100)` returns the slice
    from 200ms ago, length 100ms
  - `peek_window` with delay larger than capacity → zero-padded
    prefix + available tail (or all-zero if delay > capacity × 2)
  - `push` beyond capacity wraps (FIFO), oldest data dropped
  - Concurrent `push` + `peek_window` is safe (single-threaded async
    only — no locking required, but no torn reads either)
- `make_aec` factory
  - profile `{type: sherpa}` with sherpa installed → returns
    `SherpaAECBackend`
  - profile `{type: sherpa}` with sherpa import failing (mocked) →
    returns `NoopAECBackend`, logs warning
  - profile `{type: none}` → returns `NoopAECBackend`
  - profile `{type: echo_window}` → returns `NoopAECBackend`
    (echo_window handled separately in VAD, not via AEC backend)
- `NoopAECBackend.process_capture(mic, ref) is mic` (identity)
- `SherpaAECBackend.process_capture` shape (mocked sherpa module):
  - returns same-length int16 numpy array
  - called with correct ref slice from ring buffer
- `feed_pcm` integration
  - When `_aec_backend is NoopAECBackend`, behavior is unchanged
    from pre-AEC (regression test on existing energy/FSMN/silero paths)
  - When AEC active, mic PCM is replaced by `process_capture` output
    before VAD sees it

### Integration (`tests/test_ws_integration.py`, append)

- `test_greeting_with_aec_does_not_self_trigger`:
  - Configure `acoustic.aec: sherpa` + mocked `SherpaAECBackend` that
    zeroes input (extreme cancellation)
  - Push greeting PCM, immediately feed "phantom" mic PCM = greeting
    PCM itself (simulating pure echo)
  - Assert VAD never fires (`pcm_complete` always None)
- `test_aec_failure_falls_back_to_noop`:
  - Mock `SherpaAECBackend.init` to raise
  - Connect WS, send start, verify server log contains fallback
    warning and pipeline still works for a real utterance

### Regression

- All 110 existing tests pass unchanged (the AEC step is a no-op
  bypass when backend is `Noop`, which is what default config yields).
- `tests/test_ws_integration.py::test_speech_during_greeting_emits_barge_in_immediately`
  must keep passing — it explicitly sets `AEC_MODE=none`; with the
  new code, this also implicitly means `NoopAECBackend`. No change.

## Risks

1. **Static delay mis-calibration.** If the actual browser playback
   delay diverges from `AEC_REFERENCE_DELAY_MS=200`, the AEC filter
   sees misaligned reference and may either under-cancel (delay too
   short) or over-cancel (delay too long, eating legitimate speech).
   Mitigation: profile-tunable, measurement harness to calibrate,
   default value conservative (LiveKit uses similar).

2. **CPU cost on every mic frame.** Real AEC has non-trivial CPU cost
   (~0.5-2ms per 30ms frame on M4 Pro for WebRTC AEC3). At 16kHz
   mono this is <10% of frame budget. Mitigation: AEC is opt-in;
   users on weak hardware keep `echo_window`.

3. **sherpa-onnx AEC submodule API uncertainty.** This spec assumes
   `sherpa.onnx.Aec` exists with a `process_reference` /
   `process_capture` shape. If the actual API differs (e.g., it
   expects interleaved float32 instead of int16, or has a different
   method name), the `SherpaAECBackend` adapter absorbs the
   mismatch — that's what the adapter layer is for. Implementation
   task includes a sherpa-onnx API verification step.

4. **Adaptive filter divergence on silence.** WebRTC AEC3 can drift
   if the reference signal is silent for long stretches (between
   turns). Mitigation: the AEC library itself handles this (it
   tracks reference signal energy and pauses adaptation when
   reference is silent). Not a code-level concern.

5. **Globally-shared reference buffer cross-session contamination.**
   Two simultaneous sessions would push interleaved PCM into the
   same ring buffer, and one session's mic could see another's
   TTS as reference. In practice: voice-assistant today is
   single-session per server (one user, one browser tab). When
   multi-session is real, this becomes per-session buffers. Tracked
   in "Open Questions".

## Open Questions

1. **`send_binary_and_ref` helper vs inline push.** Four 1-liner
   push calls is the spec's choice. If measurement reveals a missed
   call site (e.g., a future TTS path forgets to push ref), a
   helper that enforces the invariant is justified. Deferred.

2. **Per-session vs global reference buffer.** Spec says global.
   Revisit when voice-assistant supports multiple concurrent
   sessions (today it doesn't; the orchestrator is single-user).

3. **Should `acoustic.aec: none` (full bypass) disable echo_window
   too?** Today `none` and `echo_window` both map to `NoopAECBackend`
   but `echo_window` keeps the VAD threshold boost. Decision: keep
   them distinct. `none` = pure off (for A/B measurement baseline);
   `echo_window` = today's default behavior.

4. **WebRtcAECBackend inclusion in initial implementation.** The
   spec lists it as a component but defers implementation until
   sherpa AEC measurement is in hand. Initial implementation
   delivers sherpa + noop; webrtc adapter is a follow-up task only
   if sherpa's ERLE is inadequate. This is YAGNI discipline, not
   exclusion.

## Out of Scope (future work)

- Browser-side reference loopback (Architecture B in brainstorming)
- Auto-calibration of static delay (run a sweep on startup, pick best)
- Room adaptation tracking (continuous ERLE measurement / display)
- Multi-channel / stereo audio
- AEC for the NeoMind Rust cdylib's own audio passthrough (the Rust
  proxy is byte-transparent; AEC happens server-side in Python)
- Vowel-specific tuning, custom transfer functions
