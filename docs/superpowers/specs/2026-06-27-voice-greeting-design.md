# Voice Assistant Greeting ("Say First" Message)

**Date:** 2026-06-27
**Status:** Design — pending implementation plan
**Scope:** `extensions/voice-assistant/` (Python orchestrator + browser)

## Motivation

When a user clicks "start" today, the session enters `ready` state and waits
for speech. The first user-perceived audio arrives only after the full
`VAD → ASR → LLM → TTS first-chunk` round trip — ~200ms on a warm
NeoMind chat WS, but perceptually it's a wall of silence right after an
explicit user gesture.

Vapi, Retell, and LiveKit all ship a **greeting** ("Say First") pattern:
on session start, immediately play a pre-authored audio clip ("Hi, I'm
the assistant") so the user gets acknowledgement in <50ms while the
actual pipeline stays dormant until they speak.

This spec defines greeting for voice-assistant's default profile path.

## Goals

1. Pre-synthesize greeting PCM at server startup so playback starts at
   ~0ms after the browser connects.
2. Push greeting as a first-class WS protocol event distinct from
   turn-lifecycle `tts_start`/`tts_end` (so existing measurement scripts
   are not contaminated).
3. Allow barge-in: user speech during greeting cancels remaining greeting
   playback and starts a real turn.
4. Default disabled (`greeting_text: ""`); opting in is a per-profile
   decision because greeting playback without AEC can self-trigger VAD.

## Non-Goals

- Randomized greeting variants (`greeting_texts: list`). YAGNI; brand
  greeting is typically fixed.
- A separate `greeting_voice` field. Uses the profile's existing TTS
  voice.
- Disk caching of synthesized greeting. Startup cost (~300ms once) is
  acceptable.
- Server-side VAD threshold hacking during greeting playback. The AEC
  gap is a pre-existing, separately-tracked issue (README "Known PoC
  limitations: AEC leakage").
- Client-side VAD or any change to the VAD backend.

## Architecture

Greeting slots into the existing startup-warmup + WS-handler patterns.
It is the third pre-synthesized PCM bank, alongside:

- `_ACK_PCM_BANK` — backchannel acks ("好的") after barge-in
- `_STAGE_FILLER_BANK` — thinking/tool_call fillers during slow LLM turns

Each bank is warmed at server startup and consumed by `ws_handler`
callbacks. Greeting follows the same shape, with one addition: a
text-frame companion for browser-side subtitle rendering.

### Component changes

| Component | Change |
|-----------|--------|
| `profile.py` | Add `greeting_text: str = ""` to `Profile` dataclass; add one line to `from_dict`'s constructor call reading `interaction.get("greeting_text", "")` (sibling to `barge_in_mode`, `ack_words`, etc. at lines 56-60) |
| `profiles/default.yaml` | Add `interaction.greeting_text: ""` (default disabled) |
| `server.py` | Add `_GREETING_PCM` module cache + `_warm_greeting()` startup hook + `_pick_greeting_pcm()` accessor; add `sess.greeting_active` flag to `VoiceSession`; on `start` frame, push greeting frame + binary if cache non-empty |
| `ws_protocol.py` | Add `encode_greeting(text: str) -> str` |
| `poc.html` | New `case 'greeting':` branch reuses subtitle renderer; PCM goes through existing playback queue unchanged |
| `measure_common.py` | Recognize `greeting` event type in `run_one_turn` but do NOT count its PCM toward `tts_chunk_count`; measurement skips greeting by design |
| `tests/` | Unit: `_warm_greeting` honors empty/non-empty text. Integration: `ws_handler` emits `ready` → `greeting` → binary when cache populated, and `ready` → no greeting when cache empty |

### Data flow

```
Server startup:
  _warm_ack_bank()          (existing)
  _warm_stage_fillers()     (existing)
  _warm_greeting(profile):  (NEW)
    if profile.greeting_text == "":
        _GREETING_PCM = None        # disabled
        return
    _GREETING_PCM = await _synth_one_shot(text, profile)

Browser connects, sends {"type":"start"}:
  ws_handler:
    send {"type":"ready", ...}                          (existing)
    if _GREETING_PCM is not None:
        sess.greeting_active = True
        sess.tts_active = True
        send {"type":"greeting","text": <text>}         (NEW)
        send_binary(_GREETING_PCM)                      (NEW)
        # no tts_start/tts_end — greeting is not a turn
    enter receive loop                                  (existing)
```

### `VoiceSession` state

Add one boolean flag:

```python
class VoiceSession:
    greeting_active: bool = False   # True between greeting push and the
                                    # first user-speech detection that
                                    # ends the greeting window
```

**Single reset trigger:** the flag flips to `False` inside the
`pcm_complete is not None` branch of `ws_handler`, **before** the
existing barge-in check at `server.py:753` and **before** starting the
new turn at line 767. Concretely, a new guard is added at the top of
that branch:

```python
pcm_complete = sess.feed_pcm(samples)
if pcm_complete is None:
    continue
# NEW: flush greeting queue immediately on first speech detection.
if sess.greeting_active:
    sess.greeting_active = False
    sess.tts_active = False
    await sess.send_json({"type": "barge_in"})  # browser flushes queue
# Existing: barge-in if a turn is in progress.
if pipeline.fsm.state in (State.THINKING, State.SPEAKING):
    ...
```

This guarantees the browser receives the flush signal within the VAD
speech-detection latency (~500ms after the user starts talking), NOT
delayed by ASR + LLM TTFB + TTS first-chunk. Without this guard, the
greeting would keep playing for 200-500ms after the user speaks, which
would defeat the goal.

The flag is consumed in **exactly one place** (the new guard above).
It is never read by `on_tts_pcm`, `on_stop_playback`, or any turn-lifecycle
callback — those callbacks operate on `sess.tts_active` / FSM state as
today.

### Barge-in frame shape

The codebase currently has two different barge-in frame shapes:

- `ws_protocol.encode_barge_in_ack()` → `{"type":"control","action":"stop_playback","reason":"barge_in"}`
- `server.py:619` (`on_stop_playback`) → `{"type":"barge_in"}` (raw)

The greeting-flush frame uses the **second** shape — `{"type":"barge_in"}`
— to match what `on_stop_playback` already emits and what `poc.html`
already handles in its `case 'barge_in':` branch. No protocol
proliferation; reuse the existing raw frame.

### Barge-in semantics during greeting

| Scenario | Behavior |
|----------|----------|
| User waits through greeting silently | Greeting plays in full; `greeting_active` stays True until next speech. Eventually user speaks → row 2 fires. |
| User speaks during greeting (interruption) | VAD detects at baseline threshold; `pcm_complete is not None` branch fires; new guard emits `{"type":"barge_in"}`, resets `greeting_active=False` and `tts_active=False`, browser flushes greeting queue remainder; existing barge-in check at line 753 sees `state == IDLE` (greeting is not a turn) → no extra cleanup; new turn starts normally at line 767. |
| Greeting self-triggers VAD (no AEC, loud speakers) | **Known limitation.** Baseline VAD threshold sees greeting audio as speech; row 2 fires on the phantom speech → greeting gets cut short by its own echo. Mitigation = enable AEC in profile (`acoustic.aec: webrtc`) or keep `greeting_text: ""`. This spec does **not** hack VAD threshold during greeting. |

The AEC gap is the same pre-existing exposure that ack and stage-filler
PCM have; greeting does not make it worse in kind, only in duration
(greeting is ~1-2s vs ack's ~300ms). Documented as opt-in.

## WS protocol addition

New server→client text frame:

```json
{"type": "greeting", "text": "你好，我是 NeoMind 助手"}
```

Followed by zero or more binary PCM frames (16kHz mono int16 LE, same
format as turn TTS). No `tts_start`/`tts_end` is emitted around
greeting — those mark turn lifecycle only.

Browser handling (`poc.html`):
- `case 'greeting':` renders `obj.text` into the subtitle area (reuses
  the `llm_sentence` rendering path).
- The binary PCM that follows goes into the existing playback queue
  unchanged.

Backward compatibility: clients that don't recognize `greeting` text
frames will ignore them (current browser-side switch has a default
`ignore`-style branch). They will still hear the binary PCM via the
existing playback queue — audio works without subtitle.

## Measurement

`measure_common.run_one_turn` is shared by `measure_bi_stream_e2e.py`
and `measure_neomind_e2e.py`. Changes:

1. `greeting` event type is recognized in the JSON-message handler but
   does NOT increment `llm_sentence_count` or any timing marker.
2. Binary PCM received **between `greeting` and the first `asr_start`**
   is counted in a new `greeting_pcm_chunks` field, NOT in
   `tts_chunk_count` / `post_tts_pcm_chunks`. This keeps the existing
   bi-stream metrics clean.

   **Determinism note:** this separation depends on the server pushing
   the greeting binary frames **synchronously** inside the `start`
   handler in `ws_handler`, before the receive loop resumes. The
   implementation MUST NOT schedule greeting push via `asyncio.create_task`
   or any deferred path — doing so would race with the harness's first
   `feed_audio` chunks and corrupt the PCM attribution. This constraint
   is part of the contract; the implementer should add an inline comment
   at the push site documenting why it's synchronous.

3. The NeoMind E2E script's preflight is unchanged — greeting is a
   presentation concern, not a NeoMind-integration concern.

The 2-silence tail that the harness already sends after the prompt will
naturally end any VAD-triggered turn that occurs after the greeting.
Greeting PCM itself is pushed by the server before the harness's first
`feed_audio` chunk is processed, so ordering is deterministic.

## Testing

### Unit (`tests/test_pipeline.py` or new `test_greeting.py`)

- `Profile.from_dict({"interaction":{"greeting_text":""}}).greeting_text == ""`
- `Profile.from_dict({}).greeting_text == ""` (default)
- `Profile.from_dict({"interaction":{"greeting_text":"你好"}}).greeting_text == "你好"`
- `_warm_greeting` with empty text → `_GREETING_PCM is None`
- `_warm_greeting` with non-empty text and mocked TTS → `_GREETING_PCM` is non-empty bytes

### Integration (`tests/test_ws_integration.py`)

- Mock TTS, set `greeting_text: "test"`, connect WS, send `start`:
  - Assert receive `ready` frame first
  - Assert receive `greeting` text frame with `text == "test"`
  - Assert receive at least one binary frame
- Set `greeting_text: ""`: connect, send `start`:
  - Assert receive `ready`
  - Assert NO `greeting` frame follows
- After greeting push, simulate first `pcm_complete` (user speech):
  - Assert `barge_in` frame is emitted to client **before** the new
    turn's first PCM
  - Assert `sess.greeting_active` resets to False
  - Assert `sess.tts_active` resets to False (so VAD threshold bumping
    and AEC windows see a clean state for the new turn)

### AEC / state interaction (`tests/test_ws_integration.py`)

- During greeting push, assert `sess.tts_active == True` (so the AEC
  echo-window logic in `VoiceSession` correctly treats greeting as
  "we are emitting audio"). This is what suppresses self-triggered VAD
  when AEC is enabled.
- After the barge-in-flush guard fires, assert both `greeting_active`
  and `tts_active` are False, so the next turn starts from a clean
  state.

### Manual / E2E

- `python server.py` with `default.yaml` (empty greeting) → connect →
  no greeting, behavior unchanged.
- Set `greeting_text: "你好，我是 NeoMind 助手"` in a test profile →
  connect → hear greeting immediately, see subtitle, then speak a real
  utterance → verify greeting stops mid-playback and the real reply
  takes over.
- `python measure_bi_stream_e2e.py --n 3` → metrics unchanged from
  Phase 2 baseline (~171ms asr→first_pcm).

## Risks

1. **AEC gap made more visible.** Greeting is 1-2s of audio pushed
   without VAD threshold bump. Phantom turn risk is real without AEC.
   Mitigation: default disabled, document AEC prerequisite.
2. **Measurement contamination.** Adding the `greeting` frame without
   excluding its PCM from `tts_chunk_count` would inflate metrics.
   Mitigation: dedicated `greeting_pcm_chunks` counter in
   `measure_common`.
3. **Browser compatibility.** Old browsers that don't handle `greeting`
   frame still hear audio (binary PCM is independent). Subtitle is the
   only lost feature; acceptable degradation.
4. **Startup delay.** Synthesizing greeting adds ~300ms to server
   startup once. Acceptable; ack and filler banks already pay similar
   costs.

## Open Questions

None — all design decisions confirmed during brainstorming:
- Pre-synth at startup (vs on-demand): **pre-synth**
- Protocol: **subtitle frame + binary, interruptible**
- Profile field shape: **single `greeting_text: str`**
- Boundary case (AEC): **option C — opt-in, no threshold hack, document**
- Default value in `default.yaml`: **empty string (disabled)**

## Out of Scope (future work)

- AEC integration (`acoustic.aec: webrtc`) — separate work item, would
  unblock safe greeting in noisy environments.
- Randomized greeting list, multi-voice greeting, disk-cached synth.
- Server-side VAD threshold bump during greeting (was option A; deferred
  until AEC is in place and we can measure whether the hack is needed
  at all).
