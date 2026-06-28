# sherpa-onnx AEC API

**Probed on:** 2026-06-28, sherpa-onnx version: **1.13.3** (`sherpa_onnx` 1.13.3 / `sherpa-onnx-core` 1.13.3, conda base env on darwin aarch64)

> Two important environment corrections vs. the task spec, both load-bearing
> for the rest of the AEC plan:
>
> 1. The installed package is **`sherpa_onnx`** (underscore), not `sherpa.onnx`
>    (dotted). `python3 -c "import sherpa.onnx"` raises `ModuleNotFoundError:
>    No module named 'sherpa'`. The voice-assistant service already imports the
>    underscore form (see `backends/vad.py:53`, `server.py:395`) so this is
>    consistent. **AEC-backend code must use `import sherpa_onnx`.**
> 2. **There is no AEC API.** See verdict below.

## Class / function name(s)

**None.** An exhaustive scan of the `sherpa_onnx` namespace returns zero
attributes whose name contains `aec`, `echo`, or `cancel`. Verified three ways:

```python
import sherpa_onnx, inspect

# 1. Top-level namespace
[n for n in dir(sherpa_onnx) if 'aec' in n.lower() or 'echo' in n.lower()]
# -> []

# 2. All public classes' methods
hits = []
for name in dir(sherpa_onnx):
    obj = getattr(sherpa_onnx, name, None)
    if inspect.isclass(obj):
        for m in dir(obj):
            if any(k in m.lower() for k in ('aec', 'echo', 'cancel')):
                hits.append(f'{name}.{m}')
hits  # -> []

# 3. Low-level C-extension shim
[n for n in dir(sherpa_onnx.lib) if 'aec' in n.lower() or 'echo' in n.lower()]
# -> []   (sherpa_onnx.lib has 0 public attrs anyway — it's a ctypes handle)
```

For completeness, the 99 public top-level attributes group as:

| Domain        | Sample classes (no AEC)                                                     |
|---------------|------------------------------------------------------------------------------|
| ASR offline   | `OfflineRecognizer`, `OfflineSenseVoiceModelConfig`, `OfflineWhisperModelConfig`, `OfflineQwen3ASRModelConfig`, … |
| ASR online    | `OnlineRecognizer`, `OnlineStream`                                           |
| VAD           | `VoiceActivityDetector`, `VadModel`, `VadModelConfig`, `SileroVadModelConfig`, `TenVadModelConfig` |
| TTS           | `OfflineTts`, `OfflineTtsKokoroModelConfig`, `OfflineTtsMatchaModelConfig`, … |
| Enhancement   | `OnlineSpeechDenoiser`, `OfflineSpeechDenoiser`, `OfflineSourceSeparation` (these are **noise/separation**, not echo cancellation — no `reference`/`far_end`/`playout` parameter anywhere) |
| Speaker / SID | `SpeakerEmbeddingExtractor`, `OfflineSpeakerDiarization`, `SpokenLanguageIdentification`, `AudioTagging` |
| I/O utilities | `write_wave`, `CircularBuffer`, `Display`, `FeatureExtractorConfig` |

## Constructor signature

N/A — no AEC class to construct.

For reference, the closest-surfaces (which Task 4 must NOT use as AEC
substitutes — they have no reference-signal channel):

- `OnlineSpeechDenoiser(config: OnlineSpeechDenoiserConfig)` — single-channel noise suppression only; `OnlineSpeechDenoiserConfig` has no `far_end` / `reference` / `loopback` field.
- `OfflineSpeechDenoiser(config: OfflineSpeechDenoiserConfig)` — offline denoise; same caveat.
- `VoiceActivityDetector(config: VadModelConfig, buffer_size_in_seconds: float)` — the existing `server.py:396` Silero VAD; no echo path.

## Frame / block size constraints

N/A — no AEC class. (For the fallback `webrtc-audio-processing-1` library the
frame constraint will be 10 ms = 160 samples @ 16 kHz, documented in Task 4-alt
instead.)

## Input dtype

N/A.

## Reference-signal API

N/A. The enhancement classes that *do* exist (`OnlineSpeechDenoiser`,
`OfflineSpeechDenoiser`) take a single capture stream — there is no
two-input `process(mic, ref)` style API anywhere in the package. This rules
out any "abuse the denoiser as a 1-tap AEC" hack.

## Output dtype and length

N/A.

## Smoke test

**Cannot be written** — there is nothing to instantiate. The 10-line probe
below is the strongest positive test possible and it returns an empty list:

```python
# smoke_probe.py — run from extensions/voice-assistant/service/
import sherpa_onnx
assert sherpa_onnx.__version__ == "1.13.3"
aec_attrs = [n for n in dir(sherpa_onnx)
             if 'aec' in n.lower() or 'echo' in n.lower() or 'cancel' in n.lower()]
print("AEC attrs:", aec_attrs)   # -> []  (verdict: API absent)
```

Run record (2026-06-28):

```
$ /opt/homebrew/Caskroom/miniconda/base/bin/python3 smoke_probe.py
AEC attrs: []
```

## Failure modes observed

1. **Primary failure: the API simply does not exist.** Not a missing-model
   problem, not a version-gated flag — there is no AEC class to instantiate
   even with a hypothetical model file. Confirmed against:
   - All 99 public top-level attrs
   - All public methods on every public class
   - The C-extension shim (`sherpa_onnx.lib`)
2. **Upstream corroboration.** sherpa-onnx's own docs and the k2-fsa issue
   tracker have no `SpeechAec`/`Aec`/`EchoCanceller` type. The closest
   existing GitHub issue (#1100 "Add speech enhancement feature") asks for
   *noise* suppression in the Adobe-Enhance-Speech style — echo
   cancellation is out of scope for the project. A third-party React Native
   wrapper mentions "echo cancellation" as a speech-enhancement bullet, but
   that is a re-export of platform-native AEC, not sherpa-onnx code.
3. **`sherpa.onnx` dotted import does not work** in this environment. Any
   example code from the sherpa docs that uses `import sherpa.onnx` is
   running in a different install configuration (likely the conda-forge
   build which registers a `sherpa` namespace package). The NeoMind voice
   service uniformly uses `import sherpa_onnx` and that is what Task 4-alt
   / Task 3+ must continue to do.
4. No model file is missing because no API consumes one.

## Verdict

**API does NOT exist.** Per the task spec:

> If the API does not exist: stop and surface to human before continuing.
> Spec says fall back to `webrtc-audio-processing-1`; that becomes Task 4-alt
> instead of Task 4 below.

**Action required from the plan owner before Tasks 3-12 proceed:**

1. **Task 4 ("SherpaAECBackend adapter") must be replaced with Task 4-alt
   ("WebRtcAECBackend adapter")** built on `webrtc-audio-processing-1`
   (a.k.a. `webrtc_audio_processing`). The current `contracts.py:91`
   `AECBackend.process(mic_pcm, reference_pcm) -> bytes` Protocol shape is
   still correct for that backend — webrtc's `EchoCanceller3` takes a
   reference (render) frame and a capture (capture) frame in the same call,
   which maps cleanly.
2. **Task 5 (`make_aec` factory) default branch** must change from
   `"sherpa"` → `"webrtc"`. The `"noop"` fallback stays.
3. **`requirements.txt`**: drop any plan to keep `sherpa-onnx` for AEC
   (it stays for VAD/ASR). Add `webrtc-audio-processing-1`.
4. **README / docs (Task 12)** must describe the webrtc backend, not sherpa.
5. **Frame constraint for Task 4-alt:** webrtc-audio-processing expects
   10-ms frames at the chosen sample rate (160 samples @ 16 kHz,
   `AudioProcessing` module insists on multiples of 10 ms). The
   `ReferenceRingBuffer` (Task 2) and `feed_pcm` hook (Task 8) must
   accumulate at least one 10-ms quantum before invoking the AEC, or the
   webrtc call will throw.

No smoke test could be run; the section above documents the strongest
positive probe instead.
