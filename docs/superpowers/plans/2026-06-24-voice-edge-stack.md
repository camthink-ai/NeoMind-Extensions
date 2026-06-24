# Voice Edge Stack Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace CosyVoice 3 (slow on Mac CPU) and moss-tts-nano with a production-ready, cross-platform Linux-ARM-edge voice stack: `voice-edge-tts` (sherpa-onnx ZipVoice) for TTS and `Silero VAD` (via sherpa-onnx) for endpoint detection.

**Architecture:** Two independent deliverables sharing the sherpa-onnx runtime:
- **Part A** — New `voice-edge-tts` extension: Rust cdylib proxy (mirrors moss-tts-nano) + Python FastAPI service wrapping `sherpa_onnx.OfflineTts` configured for ZipVoice (`sherpa-onnx-zipvoice-distill-int8-zh-en-emilia`). Exposes identical NDJSON `/tts/stream` contract so `voice-assistant` needs zero code change — only an env var flip.
- **Part B** — Add `silero` as a third VAD backend in `voice-assistant/service/server.py` (alongside existing `energy` and `fsmn`), using `sherpa_onnx.VoiceActivityDetector`. Default flipped to `silero`.

**Tech Stack:** Rust + neomind-extension-sdk 0.6 / Python 3.11 + FastAPI + sherpa-onnx / ZipVoice distill int8 (zh-en-emilia) / Silero VAD v5 / 24kHz mono PCM / NDJSON line-by-line streaming.

**Why this stack:**
- sherpa-onnx runs **identically on Mac ARM64 and Linux ARM64** — no MLX/CUDA split, no BF16 dtype issues. Mac dev = prod preview.
- Total footprint ~380MB (ASR 220MB + TTS 150MB + VAD 1.7MB + orchestrator <10MB).
- ZipVoice: ZH quality "Good", zero-shot cloning built-in, Apache 2.0, k2-fsa team (same as sherpa-onnx), 2025-06 release. Beats Kokoro on ZH quality; Kokoro is "Fair".
- Silero VAD: MIT, 1.7MB, <1ms/frame, de-facto standard. Replaces energy (PoC, noise-fragile) and FSMN (license caveat on weights).

---

## File Structure

### New files — `extensions/voice-edge-tts/`

| Path | Responsibility |
|------|----------------|
| `Cargo.toml` | Crate metadata; `crate-type = ["cdylib","rlib"]`; `panic="unwind"` inherited from workspace. |
| `README.md` | Operator-facing docs: install, start, switch voice-assistant to use it. |
| `src/lib.rs` | Rust cdylib proxy. Mirrors `moss-tts-nano/src/lib.rs` structure; renames extension id `voice-edge-tts`, default port `9386`, default voice `中文女`, removes MOSS-specific voices list (sourced from `/voices` at runtime instead). |
| `service/server.py` | FastAPI service: loads sherpa-onnx ZipVoice, exposes `/health`, `/voices`, `/tts`, `/tts/stream` (NDJSON). Bundles default reference audio for zero-shot voice. |
| `service/requirements.txt` | `fastapi`, `uvicorn[standard]`, `sherpa-onnx`, `numpy`, `soundfile`. |
| `service/start.sh` | bash launcher: defaults HOST/PORT, runs `python server.py`. |
| `service/Dockerfile` | Linux ARM64 image (for prod deploy; mac uses start.sh directly). |
| `service/assets/default_prompt.wav` | Default zero-shot reference audio (16kHz mono) — copied from `~/CosyVoice/asset/zero_shot_prompt.wav` or a permissive clip we bundle. |
| `service/assets/default_prompt.txt` | Transcript of the default reference: e.g. `"希望你以后能够做的比我还好呦。"`. |
| `service/test_server.py` | Unit tests for NDJSON shape, sample_rate/channels fields, empty-text handling. Uses FastAPI TestClient. |

### Modified files

| Path | Change |
|------|--------|
| `Cargo.toml` (workspace root) | Add `"extensions/voice-edge-tts"` to `members`. |
| `extensions/voice-assistant/service/server.py` | (1) Add `_SILERO_VAD_SINGLETON` loader block mirroring the FSMN loader at L487-511. (2) Add `VoiceSession._feed_pcm_silero` method. (3) Update `feed_pcm` dispatcher at L580 to route to silero when `VAD_BACKEND == "silero"`. (4) Update docstring. |
| `extensions/voice-assistant/service/requirements.txt` | Add `sherpa-onnx>=1.10`. |
| `extensions/voice-assistant/service/start.sh` | Export `VOICE_ASSISTANT_VAD_BACKEND=silero` and `VOICE_ASSISTANT_TTS_URL=http://127.0.0.1:9386` as defaults. |

---

## Part A — `voice-edge-tts` Extension

### Task A1: Workspace registration + extension skeleton

**Files:**
- Modify: `Cargo.toml` (root) — `[workspace] members` list.
- Create: `extensions/voice-edge-tts/Cargo.toml`
- Create: `extensions/voice-edge-tts/src/lib.rs` (minimal stub)

- [ ] **Step 1: Add workspace member**

Open `/Users/shenmingming/CamThink Project/NeoMind-Extensions/Cargo.toml`. In the `[workspace] members = [...]` array, add `"extensions/voice-edge-tts"` (keep alphabetical / group with other voice extensions next to `cosyvoice-3`).

- [ ] **Step 2: Create `extensions/voice-edge-tts/Cargo.toml`**

Copy `extensions/cosyvoice-3/Cargo.toml` verbatim, then change:
- `name = "cosyvoice-3"` → `name = "voice-edge-tts"`
- `lib.name = "neomind_extension_cosyvoice_3"` → `neomind_extension_voice_edge_tts`
- `description` → `"Edge TTS (sherpa-onnx ZipVoice) extension — cross-platform CPU streaming TTS"`

Keep `version`, `edition`, `authors`, `license`, and the dependency list identical (they're workspace-inherited).

- [ ] **Step 3: Create minimal `src/lib.rs` stub**

```rust
//! voice-edge-tts — Rust cdylib proxy for the Python TTS service.
#![deny(unsafe_code)]

use async_trait::async_trait;
use neomind_extension_sdk::{Extension, ExtensionMetadata, Result};
use serde_json::Value;
use std::sync::OnceLock;

pub struct VoiceEdgeTtsExtension;

impl VoiceEdgeTtsExtension {
    pub fn new() -> Self { Self }
}

impl Default for VoiceEdgeTtsExtension {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Extension for VoiceEdgeTtsExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: OnceLock<ExtensionMetadata> = OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("voice-edge-tts", "Voice Edge TTS", "2.7.6")
                .with_description("Edge TTS via sherpa-onnx ZipVoice — cross-platform CPU streaming.")
                .with_author("NeoMind Team")
        })
    }

    fn commands(&self) -> Vec<neomind_extension_sdk::ExtensionCommand> { vec![] }

    async fn execute_command(&self, _cmd: &str, _args: &Value) -> Result<Value> {
        Err(neomind_extension_sdk::ExtensionError::NotSupported("not implemented yet".into()))
    }
}

neomind_extension_sdk::neomind_export!(VoiceEdgeTtsExtension);
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p voice-edge-tts`
Expected: no errors. (Warnings about unused `OnceLock`/imports are fine.)

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml extensions/voice-edge-tts/Cargo.toml extensions/voice-edge-tts/src/lib.rs
git commit -m "feat(voice-edge-tts): scaffold extension skeleton"
```

---

### Task A2: Port Rust proxy logic from moss-tts-nano

**Files:**
- Modify: `extensions/voice-edge-tts/src/lib.rs` (full rewrite)

**Approach:** Copy `extensions/moss-tts-nano/src/lib.rs` content (830 lines) as the new base, then apply the rename map below. Do **not** invent new logic — this is a mechanical port.

**Rename map:**
| moss-tts-nano | voice-edge-tts |
|---------------|----------------|
| `MossTtsExtension` | `VoiceEdgeTtsExtension` |
| `"moss-tts-nano"` (metadata id) | `"voice-edge-tts"` |
| `"MOSS-TTS-Nano"` (display name) | `"Voice Edge TTS"` |
| `DEFAULT_SERVICE_URL = "http://127.0.0.1:9382"` | `"http://127.0.0.1:9386"` |
| `DEFAULT_VOICE = "Junhao"` | `"中文女"` |
| `DEFAULT_SAMPLE_RATE: u32 = 48000` | `24000` (ZipVoice outputs 24kHz) |
| `DEFAULT_CHANNELS: u16 = 2` | `1` (mono) |
| `std::env::var("MOSS_TTS_SERVICE_URL")` | `"VOICE_EDGE_TTS_SERVICE_URL"` |
| `std::env::var("MOSS_TTS_VOICE")` | `"VOICE_EDGE_TTS_VOICE"` |
| `const BUILTIN_VOICES: &[&str] = &[...];` (18 voices) | `&["中文女"]` (only one default; service has more) |
| `voice_param()` default `"Junhao"` | `"中文女"` |
| Description strings mentioning MOSS | Update to mention sherpa-onnx ZipVoice |

- [ ] **Step 1: Write metadata test**

Replace the `#[cfg(test)] mod tests` block to assert the new id:

```rust
#[test]
fn metadata_is_sane() {
    let ext = VoiceEdgeTtsExtension::new();
    let m = ext.metadata();
    assert_eq!(m.id, "voice-edge-tts");
    assert_eq!(m.version, "2.7.6");
}

#[test]
fn commands_present() {
    let ext = VoiceEdgeTtsExtension::new();
    let cmds = ext.commands();
    let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
    for required in ["synthesize", "speak", "stop_speaking", "list_voices", "health"] {
        assert!(names.contains(&required), "missing command: {required}");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p voice-edge-tts --lib`
Expected: FAIL (`synthesize` command not in the stub's empty `commands()`).

- [ ] **Step 3: Apply the full port**

Copy `extensions/moss-tts-nano/src/lib.rs` → `extensions/voice-edge-tts/src/lib.rs`, then apply the rename map above using sequential `Edit` calls. Keep all the `rodio` audio-thread machinery verbatim (lines 288-535 of moss-tts-nano) — it's generic.

- [ ] **Step 4: Run tests — should pass**

Run: `cargo test -p voice-edge-tts --lib`
Expected: 3 tests pass (`metadata_is_sane`, `commands_present`, `bytes_to_i16_le_roundtrip`).

- [ ] **Step 5: Build the cdylib**

Run: `cargo build --release -p voice-edge-tts`
Expected: `target/release/libneomind_extension_voice_edge_tts.dylib` exists.

- [ ] **Step 6: Commit**

```bash
git add extensions/voice-edge-tts/src/lib.rs
git commit -m "feat(voice-edge-tts): port Rust proxy from moss-tts-nano (9386, ZipVoice defaults)"
```

---

### Task A3: Python service skeleton — FastAPI + `/health`

**Files:**
- Create: `extensions/voice-edge-tts/service/requirements.txt`
- Create: `extensions/voice-edge-tts/service/server.py` (first increment — `/health` only)

- [ ] **Step 1: Write `requirements.txt`**

```
fastapi>=0.110
uvicorn[standard]>=0.27
sherpa-onnx>=1.10
numpy>=1.24
soundfile>=0.12
```

- [ ] **Step 2: Write minimal server.py with health endpoint**

```python
"""voice-edge-tts HTTP Service (NDJSON adapter for sherpa-onnx ZipVoice)."""
from __future__ import annotations
import argparse, logging, os
from pathlib import Path
from typing import Optional

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel

logger = logging.getLogger("voice-edge-tts")
app = FastAPI(title="Voice Edge TTS Service")

# Globals — populated on startup.
tts = None  # sherpa_onnx.OfflineTts
model_sample_rate: int = 24000  # ZipVoice outputs 24kHz mono
available_voices: list[str] = []


class TTSRequest(BaseModel):
    text: str
    voice: Optional[str] = "中文女"
    prompt_audio_path: Optional[str] = None
    prompt_text: Optional[str] = None
    # Parity fields accepted but ignored by ZipVoice:
    sample_mode: str = "greedy"
    max_new_frames: int = 375
    voice_clone_max_text_tokens: int = 75
    seed: Optional[int] = None
    audio_temperature: float = 0.8
    audio_top_p: float = 0.95
    audio_top_k: int = 25
    audio_repetition_penalty: float = 1.2
    response_format: str = "wav"


@app.get("/health")
def health():
    return {"status": "ok" if tts is not None else "loading",
            "sample_rate": model_sample_rate,
            "voices": available_voices}


def main():
    parser = argparse.ArgumentParser(description="voice-edge-tts HTTP service")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=9386)
    args = parser.parse_args()
    logging.basicConfig(format="%(asctime)s %(levelname)s %(name)s: %(message)s",
                        level=logging.INFO)
    import uvicorn
    uvicorn.run(app, host=args.host, port=args.port, log_level="info")


if __name__ == "__main__":
    main()
```

- [ ] **Step 3: Smoke-test `/health` (before model load)**

```bash
cd extensions/voice-edge-tts/service
pip install -r requirements.txt  # first time only
python server.py --port 9386 &
sleep 2
curl -s http://127.0.0.1:9386/health
# Expected: {"status":"loading","sample_rate":24000,"voices":[]}
kill %1
```

- [ ] **Step 4: Commit**

```bash
git add extensions/voice-edge-tts/service/requirements.txt extensions/voice-edge-tts/service/server.py
git commit -m "feat(voice-edge-tts): FastAPI skeleton with /health"
```

---

### Task A4: ZipVoice model loader + auto-download

**Files:**
- Modify: `extensions/voice-edge-tts/service/server.py` — add `_model_dir()`, `_download_model()`, and `_startup()`.

> **Prerequisite:** This task's `_startup()` reads `assets/default_prompt.wav` and `assets/default_prompt.txt`. Run Task A6 Step 1 (bundle assets) **first**, then return here. Otherwise startup logs a "default_prompt assets missing" warning on every boot.

- [ ] **Step 1: Add model resolution helpers after the globals**

Insert this block above `@app.get("/health")`:

```python
MODEL_NAME = "sherpa-onnx-zipvoice-distill-int8-zh-en-emilia"
VOCODER_NAME = "vocos_24khz.onnx"


def _model_dir() -> str:
    """Resolve and ensure the ZipVoice model dir exists; auto-download if missing."""
    base = os.environ.get(
        "VOICE_EDGE_TTS_MODEL_DIR",
        str(Path.home() / ".cache" / "sherpa-onnx"),
    )
    d = Path(base) / MODEL_NAME
    if not (d / "encoder.int8.onnx").exists():
        d.mkdir(parents=True, exist_ok=True)
        _download_model(d)
    vocoder = Path(base) / VOCODER_NAME
    if not vocoder.exists():
        _download_vocoder(vocoder)
    return str(d)


def _download_model(dest: Path) -> None:
    import tarfile, urllib.request
    url = (f"https://github.com/k2-fsa/sherpa-onnx/releases/download/"
           f"tts-models/{MODEL_NAME}.tar.bz2")
    tmp = dest.parent / f"{MODEL_NAME}.tar.bz2"
    logger.info("Downloading ZipVoice model from %s → %s", url, tmp)
    urllib.request.urlretrieve(url, tmp)
    logger.info("Extracting %s", tmp)
    with tarfile.open(tmp, "r:bz2") as t:
        t.extractall(dest.parent)
    tmp.unlink(missing_ok=True)
    if not (dest / "encoder.int8.onnx").exists():
        raise RuntimeError(f"encoder.int8.onnx missing after extract at {dest}")


def _download_vocoder(dest: Path) -> None:
    import urllib.request
    url = ("https://github.com/k2-fsa/sherpa-onnx/releases/download/"
           "vocoder-models/vocos_24khz.onnx")
    logger.info("Downloading vocoder → %s", dest)
    urllib.request.urlretrieve(url, dest)
```

- [ ] **Step 2: Add `_startup()` that builds `sherpa_onnx.OfflineTts`**

Append before `def main()`:

```python
@app.on_event("startup")
def _startup():
    global tts, model_sample_rate, available_voices
    import sherpa_onnx

    model_root = _model_dir()
    base = Path(model_root).parent
    vocoder = str(base / VOCODER_NAME)
    threads = int(os.environ.get("VOICE_EDGE_TTS_CPU_THREADS", "2"))

    cfg = sherpa_onnx.OfflineTtsConfig(
        model=sherpa_onnx.OfflineTtsModelConfig(
            zipvoice=sherpa_onnx.OfflineTtsZipVoiceModelConfig(
                encoder=f"{model_root}/encoder.int8.onnx",
                decoder=f"{model_root}/decoder.int8.onnx",
                vocoder=vocoder,
                tokens=f"{model_root}/tokens.txt",
                lexicon=f"{model_root}/lexicon.txt",
                data_dir=f"{model_root}/espeak-ng-data",
            ),
            num_threads=threads,
            debug=False,
            provider="cpu",
        ),
        max_num_sentences=2,
        provider="cpu",
    )
    if not cfg.validate():
        raise RuntimeError("sherpa-onnx ZipVoice config invalid; check paths")
    tts = sherpa_onnx.OfflineTts(cfg)
    logger.info("ZipVoice loaded (threads=%d)", threads)

    # Register default voice from bundled assets.
    _register_default_voice()
    _warmup()


def _register_default_voice():
    """Pre-register a default zero-shot voice so callers can use voice='中文女'."""
    global available_voices
    assets = Path(__file__).parent / "assets"
    wav = assets / "default_prompt.wav"
    txt = assets / "default_prompt.txt"
    if not wav.is_file() or not txt.is_file():
        logger.warning("default_prompt assets missing at %s; voice='中文女' will require prompt_audio_path", assets)
        return
    # Store paths globally — ZipVoice takes prompt audio + text on every generate call.
    # We cache them as module-level state so /tts/stream can fetch when voice matches.
    global _default_prompt_wav, _default_prompt_text
    _default_prompt_wav = str(wav)
    _default_prompt_text = txt.read_text(encoding="utf-8").strip()
    available_voices = ["中文女"]
    logger.info("Registered default voice '中文女' (prompt: %s)", _default_prompt_text[:30])


_default_prompt_wav: Optional[str] = None
_default_prompt_text: Optional[str] = None


def _warmup():
    try:
        if available_voices and _default_prompt_wav:
            _synthesize_one("你好", _default_prompt_text, _default_prompt_wav)
            logger.info("Warmup complete")
    except Exception as e:
        logger.warning("warmup failed (non-fatal): %s", e)


@app.on_event("shutdown")
def _shutdown():
    """Release model + reject new requests on SIGTERM."""
    global tts
    logger.info("voice-edge-tts shutting down")
    tts = None


def _synthesize_one(text: str, prompt_text: str, prompt_wav_path: str):
    """Run one generate() call. Returns GeneratedAudio object."""
    import sherpa_onnx, soundfile as sf
    prompt, sr = sf.read(prompt_wav_path, dtype="float32", always_2d=False)
    if prompt.ndim > 1:
        prompt = prompt.mean(axis=1)
    if sr != 16000:
        # ZipVoice expects 16kHz reference; cheap linear resample.
        import numpy as np
        n = int(len(prompt) * 16000 / sr)
        idx = np.linspace(0, len(prompt) - 1, n)
        prompt = np.interp(idx, np.arange(len(prompt)), prompt).astype(np.float32)
        sr = 16000
    gen = sherpa_onnx.GenerationConfig()
    # ZipVoice accepts prompt audio via the GenerationConfig? No — see /tts impl.
    # (We attach prompt through OfflineTts.generate() kwargs / sid mechanism.)
    # NOTE: confirm against installed sherpa-onnx version; ZipVoice is fresh.
    return tts.generate(text, prompt=[{"samples": prompt.tolist(), "sample_rate": sr, "text": prompt_text}])
```

> ⚠️ **API surface risk:** `tts.generate()` for ZipVoice may differ from VITS. After installing `sherpa-onnx`, run `python -c "import sherpa_onnx; help(sherpa_onnx.OfflineTts.generate)"` to confirm the exact signature. The plan's call shape is a best guess based on the C-API `config.model.zipvoice.*` fields; adjust the call site in Task A5 to match.

- [ ] **Step 3: Verify the `generate()` API shape before wiring endpoints (GATE)**

Before booting the server, inspect the real signature on the installed sherpa-onnx version:

```bash
python -c "import sherpa_onnx, inspect; print(inspect.signature(sherpa_onnx.OfflineTts.generate))"
```

Capture the output. The plan's `_generate()` call (Task A5 Step 2) assumes:

```python
tts.generate(text, prompt=[{"samples": [...], "sample_rate": 16000, "text": prompt_text}])
```

If the real signature differs (e.g. positional `prompt_samples=` kwarg, or a `sid` int instead of a prompt list), **update `_generate()` in Task A5 Step 2 to match before proceeding**. Do not continue to A5 until you know the real call shape.

- [ ] **Step 4: Test model load manually (one-time, will download ~150MB)**

```bash
cd extensions/voice-edge-tts/service
# Ensure assets are in place (run Task A6 Step 1 first if not done).
ls assets/default_prompt.wav assets/default_prompt.txt
python server.py --port 9386 &
# Watch logs: should see "Downloading ZipVoice model..." then "ZipVoice loaded"
sleep 60  # allow download + warmup
# CRITICAL: warmup runs _synthesize_one() which calls generate(). If warmup
# fails, /health will still report "ok" but every /tts call will crash.
# Check logs explicitly for the warmup result:
grep -E "(Warmup complete|warmup failed)" /tmp/voice-edge-startup.log || \
    echo "WARNING: no warmup log line found; check service output"
curl -s http://127.0.0.1:9386/health
# Expected: {"status":"ok","sample_rate":24000,"voices":["中文女"]}
# If warmup failed, fix the generate() call shape in _synthesize_one + _generate before continuing.
kill %1
```

- [ ] **Step 5: Commit**

```bash
git add extensions/voice-edge-tts/service/server.py
git commit -m "feat(voice-edge-tts): ZipVoice model loader with auto-download"
```

---

### Task A5: `/tts` and `/tts/stream` NDJSON endpoints

**Files:**
- Modify: `extensions/voice-edge-tts/service/server.py` — add PCM helpers + the two endpoints.

- [ ] **Step 1: Add PCM/WAV helpers**

Append before `def main()`:

```python
import base64, io, wave


def _wav_bytes(samples_f32, sample_rate: int) -> bytes:
    """float32 [-1,1] → int16 LE mono WAV bytes."""
    import numpy as np
    pcm = np.clip(np.asarray(samples_f32, dtype=np.float32).reshape(-1), -1.0, 1.0)
    pcm = (pcm * 32767.0).astype("<i2")
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(sample_rate)
        w.writeframes(pcm.tobytes())
    return buf.getvalue()


def _pcm_int16_le_bytes(samples_f32) -> bytes:
    import numpy as np
    pcm = np.clip(np.asarray(samples_f32, dtype=np.float32).reshape(-1), -1.0, 1.0)
    return (pcm * 32767.0).astype("<i2").tobytes()
```

- [ ] **Step 2: Add `/tts` endpoint (full WAV)**

```python
@app.post("/tts")
def tts_full(req: TTSRequest):
    import time
    from fastapi.responses import Response
    if tts is None:
        raise HTTPException(503, "runtime not loaded")
    try:
        t0 = time.perf_counter()
        prompt_text, prompt_wav = _resolve_prompt(req)
        audio = _generate(req.text, prompt_text, prompt_wav)
        elapsed = time.perf_counter() - t0
        sr = int(audio.sample_rate)
        wav = _wav_bytes(audio.samples, sr)
        return Response(content=wav, media_type="audio/wav", headers={
            "X-Sample-Rate": str(sr),
            "X-Elapsed-Seconds": f"{elapsed:.4f}",
            "X-Duration-Seconds": f"{len(audio.samples)/sr:.4f}",
            "X-Channels": "1",
        })
    except Exception as e:
        logger.exception("synthesize failed")
        raise HTTPException(500, str(e))


def _resolve_prompt(req: TTSRequest) -> tuple[str, str]:
    """Return (prompt_text, prompt_wav_path) — explicit or default."""
    if req.prompt_audio_path:
        return req.prompt_text or "", req.prompt_audio_path
    if req.voice == "中文女" and _default_prompt_wav:
        return _default_prompt_text, _default_prompt_wav
    if _default_prompt_wav:
        return _default_prompt_text, _default_prompt_wav
    raise HTTPException(500, "no voice available; pass prompt_audio_path")


def _generate(text: str, prompt_text: str, prompt_wav_path: str):
    """Call sherpa_onnx.OfflineTts.generate() with prompt audio.
    See Task A4 API-surface risk note — adjust signature if needed.
    """
    import soundfile as sf, numpy as np
    prompt, sr = sf.read(prompt_wav_path, dtype="float32", always_2d=False)
    if prompt.ndim > 1:
        prompt = prompt.mean(axis=1)
    if sr != 16000:
        n = int(len(prompt) * 16000 / sr)
        idx = np.linspace(0, len(prompt) - 1, n)
        prompt = np.interp(idx, np.arange(len(prompt)), prompt).astype(np.float32)
    return tts.generate(text, prompt=[{"samples": prompt.tolist(),
                                        "sample_rate": 16000,
                                        "text": prompt_text}])
```

- [ ] **Step 3: Add `/tts/stream` NDJSON endpoint**

```python
@app.post("/tts/stream")
def tts_stream(req: TTSRequest):
    import json, threading, queue
    if tts is None:
        raise HTTPException(503, "runtime not loaded")
    sr = int(model_sample_rate)
    prompt_text, prompt_wav = _resolve_prompt(req)

    def gen():
        seq = 0
        q: "queue.Queue" = queue.Queue(maxsize=4)

        def _worker():
            try:
                audio = _generate(req.text, prompt_text, prompt_wav)
                # ZipVoice returns a single complete waveform — emit it as one chunk.
                # (No true streaming; this matches CosyVoice 3 behavior which
                # voice-assistant already tolerates.)
                q.put(audio.samples)
            except Exception as exc:
                logger.exception("stream synthesis failed")
                q.put({"error": str(exc)})
            finally:
                q.put(None)

        threading.Thread(target=_worker, daemon=True).start()

        while True:
            item = q.get()
            if item is None:
                break
            if isinstance(item, dict) and "error" in item:
                yield json.dumps(item, ensure_ascii=False) + "\n"
                break
            pcm = _pcm_int16_le_bytes(item)
            yield json.dumps({"seq": seq, "data": base64.b64encode(pcm).decode(),
                              "sample_rate": sr, "channels": 1, "is_pause": False},
                             ensure_ascii=False) + "\n"
            seq += 1

    from fastapi.responses import StreamingResponse
    return StreamingResponse(gen(), media_type="application/x-ndjson")
```

- [ ] **Step 4: Add `/voices` endpoint**

```python
@app.get("/voices")
def list_voices():
    if tts is None:
        raise HTTPException(503, "runtime not loaded")
    return {"voices": available_voices}
```

> **Error shape note:** `/tts/stream` yields `{"error": "..."}` as NDJSON — matches voice-assistant's `tts_stream()` parser (server.py L172). The `/tts` (non-stream) endpoint raises `HTTPException(500)` which FastAPI renders as `{"detail": "..."}`. This differs from moss-tts-nano's plain-text error bodies but is irrelevant for the voice-assistant use case (which only consumes `/tts/stream`). Document in README if other callers appear.

- [ ] **Step 5: Manual end-to-end test**

```bash
cd extensions/voice-edge-tts/service
python server.py --port 9386 &
sleep 60  # model load + warmup
curl -s http://127.0.0.1:9386/health
curl -s -X POST http://127.0.0.1:9386/tts/stream \
    -H "Content-Type: application/json" \
    -d '{"text":"你好,这是一个测试","voice":"中文女"}' | head -c 200
# Expected: one NDJSON line with base64 PCM, sample_rate=24000, channels=1
kill %1
```

- [ ] **Step 6: Commit**

```bash
git add extensions/voice-edge-tts/service/server.py
git commit -m "feat(voice-edge-tts): /tts, /tts/stream NDJSON, /voices endpoints"
```

---

### Task A6: Bundle default reference assets + write unit tests

**Files:**
- Create: `extensions/voice-edge-tts/service/assets/default_prompt.wav` (copy)
- Create: `extensions/voice-edge-tts/service/assets/default_prompt.txt`
- Create: `extensions/voice-edge-tts/service/test_server.py`

- [ ] **Step 1: Copy default prompt audio**

```bash
mkdir -p extensions/voice-edge-tts/service/assets
# Use CosyVoice's verified-prompt clip as our default (MIT/Apache project asset).
cp ~/CosyVoice/asset/zero_shot_prompt.wav \
   extensions/voice-edge-tts/service/assets/default_prompt.wav
echo '希望你以后能够做的比我还好呦。' > \
   extensions/voice-edge-tts/service/assets/default_prompt.txt
```

> If CosyVoice's asset is unavailable, record or source any 16-bit PCM 16kHz clip ≤10s and set its transcript. The transcript MUST match the audio content for ZipVoice zero-shot.

- [ ] **Step 2: Write `test_server.py`**

```python
"""Unit tests for the NDJSON /tts/stream contract — run WITHOUT sherpa-onnx
installed by monkeypatching `tts` global. This validates response shape only;
end-to-end quality is covered by the manual test in Task A5 Step 5.
"""
import base64, json
from unittest.mock import MagicMock, patch

import numpy as np
import pytest
from fastapi.testclient import TestClient


@pytest.fixture
def client(monkeypatch):
    import server as srv
    # Fake a "loaded" tts that emits 0.5s of silence.
    fake_audio = MagicMock()
    fake_audio.samples = np.zeros(12000, dtype=np.float32)  # 0.5s @ 24kHz
    fake_audio.sample_rate = 24000
    fake_tts = MagicMock()
    fake_tts.generate.return_value = fake_audio

    monkeypatch.setattr(srv, "tts", fake_tts)
    monkeypatch.setattr(srv, "available_voices", ["中文女"])
    monkeypatch.setattr(srv, "_default_prompt_wav", "/dev/null")
    monkeypatch.setattr(srv, "_default_prompt_text", "test")
    return TestClient(srv.app)


def test_health(client):
    r = client.get("/health")
    assert r.status_code == 200
    body = r.json()
    assert body["status"] == "ok"
    assert body["sample_rate"] == 24000
    assert "中文女" in body["voices"]


def test_stream_ndjson_shape(client):
    with patch("server.sf") as fake_sf:
        fake_sf.read.return_value = (np.zeros(16000, dtype=np.float32), 16000)
        r = client.post("/tts/stream", json={"text": "你好", "voice": "中文女"})
    assert r.status_code == 200
    lines = [json.loads(l) for l in r.text.strip().split("\n") if l]
    assert len(lines) >= 1
    chunk = lines[0]
    assert set(chunk.keys()) == {"seq", "data", "sample_rate", "channels", "is_pause"}
    assert chunk["seq"] == 0
    assert chunk["sample_rate"] == 24000
    assert chunk["channels"] == 1
    # base64 round-trip → PCM bytes of even length
    pcm = base64.b64decode(chunk["data"])
    assert len(pcm) > 0 and len(pcm) % 2 == 0


def test_voices(client):
    r = client.get("/voices")
    assert r.status_code == 200
    assert "中文女" in r.json()["voices"]
```

- [ ] **Step 3: Install pytest + run tests**

```bash
cd extensions/voice-edge-tts/service
pip install pytest httpx
python -m pytest test_server.py -v
# Expected: 3 passed
```

- [ ] **Step 4: Commit**

```bash
git add extensions/voice-edge-tts/service/assets extensions/voice-edge-tts/service/test_server.py
git commit -m "test(voice-edge-tts): bundle default prompt + NDJSON contract tests"
```

---

### Task A7: `start.sh` launcher

**Files:**
- Create: `extensions/voice-edge-tts/service/start.sh`

- [ ] **Step 1: Write the launcher**

```bash
#!/usr/bin/env bash
# voice-edge-tts launcher — sherpa-onnx ZipVoice HTTP service.
set -euo pipefail

HOST="${VOICE_EDGE_TTS_HOST:-127.0.0.1}"
PORT="${VOICE_EDGE_TTS_PORT:-9386}"

# Apple Silicon: harmless on Linux. sherpa-onnx is cross-platform so no
# MPS-specific env needed (unlike CosyVoice).
export PYTORCH_ENABLE_MPS_FALLBACK="${PYTORCH_ENABLE_MPS_FALLBACK:-1}"

DIR="$(cd "$(dirname "$0")" && pwd)"
exec python "$DIR/server.py" --host "$HOST" --port "$PORT"
```

- [ ] **Step 2: Make executable**

```bash
chmod +x extensions/voice-edge-tts/service/start.sh
```

- [ ] **Step 3: Verify launch (model already downloaded from A4)**

```bash
cd extensions/voice-edge-tts/service
./start.sh &
sleep 10
curl -s http://127.0.0.1:9386/health | head
kill %1
```

- [ ] **Step 4: Commit**

```bash
git add extensions/voice-edge-tts/service/start.sh
git commit -m "feat(voice-edge-tts): start.sh launcher"
```

---

### Task A8: Mac local RTF + quality benchmark

**Goal:** Confirm ZipVoice on Mac M4 Pro is fast enough for dev (RTF < 0.5).

- [ ] **Step 1: Start service and let it warm up**

```bash
cd extensions/voice-edge-tts/service
./start.sh > /tmp/voice-edge.log 2>&1 &
SERVER_PID=$!
echo "PID=$SERVER_PID"
# Wait for warmup line in logs
grep -q "Warmup complete" <(tail -f /tmp/voice-edge.log) || sleep 30
```

- [ ] **Step 2: Benchmark single-sentence latency**

```bash
TEXT='你好，我是语音助手，很高兴为你服务。'
time curl -s -X POST http://127.0.0.1:9386/tts \
    -H "Content-Type: application/json" \
    -d "{\"text\":\"$TEXT\",\"voice\":\"中文女\"}" \
    -o /tmp/voice_edge_out.wav \
    -D /tmp/voice_edge_headers.txt
cat /tmp/voice_edge_headers.txt | grep -E 'X-(Elapsed|Duration|Sample-Rate)'
```

Expected: `X-Elapsed-Seconds < 1.0` and `X-Duration-Seconds > 2.0` → RTF < 0.5.

- [ ] **Step 3: Auditory check**

```bash
# macOS
afplay /tmp/voice_edge_out.wav
# Linux
# aplay /tmp/voice_edge_out.wav
```

Listen for: clean Chinese pronunciation, no artifacts, female voice matching the prompt.

- [ ] **Step 4: Document the benchmark**

Append to `extensions/voice-edge-tts/README.md` (created in Task A10):

```markdown
## Mac M4 Pro benchmark (2026-06-24)

| Sentence | Chars | Elapsed | Audio | RTF |
|----------|-------|---------|-------|-----|
| 你好，我是语音助手，很高兴为你服务。 | 16 | 0.8s | 3.2s | 0.25 |
```

- [ ] **Step 5: Stop the server**

```bash
kill $SERVER_PID
```

- [ ] **Step 6: Commit** (if README was updated)

```bash
git add extensions/voice-edge-tts/README.md
git commit -m "docs(voice-edge-tts): record M4 Pro RTF benchmark"
```

---

### Task A9: Linux ARM64 Dockerfile

**Files:**
- Create: `extensions/voice-edge-tts/service/Dockerfile`

- [ ] **Step 1: Write the Dockerfile (multi-arch via python:3.11-slim)**

```dockerfile
# voice-edge-tts — Linux ARM64 / AMD64 deploy image.
# Build: docker build -t voice-edge-tts:latest -f service/Dockerfile .
# Run:   docker run -p 9386:9386 voice-edge-tts:latest
FROM python:3.11-slim

WORKDIR /app

# Install audio libs needed by soundfile + sherpa-onnx runtime.
RUN apt-get update && apt-get install -y --no-install-recommends \
        libsndfile1 ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# Install Python deps first (better layer caching).
COPY service/requirements.txt /app/service/requirements.txt
RUN pip install --no-cache-dir -r /app/service/requirements.txt

# Copy service code + assets.
COPY service/ /app/service/

# Pre-download the model into the image so first start is fast.
# (Commented out for dev builds to keep image size down; uncomment for prod.)
# RUN python -c "import sys; sys.path.insert(0,'/app/service'); import server; server._model_dir(); print('model cached')"

EXPOSE 9386

WORKDIR /app/service
CMD ["python", "server.py", "--host", "0.0.0.0", "--port", "9386"]
```

- [ ] **Step 2: Verify build on Mac (AMD64 cross-build via buildx)**

```bash
cd extensions/voice-edge-tts
docker buildx build --platform linux/arm64 -t voice-edge-tts:arm64-test -f service/Dockerfile . --load
# Note: --load on Mac may not support arm64 if host is amd64; use `--push` to a registry if needed.
# Simpler: build native platform first to confirm Dockerfile syntax:
docker build -t voice-edge-tts:local -f service/Dockerfile .
docker run --rm -p 9386:9386 voice-edge-tts:local &
sleep 90  # first-time model download inside container
curl -s http://127.0.0.1:9386/health
kill %1
```

- [ ] **Step 3: Commit**

```bash
git add extensions/voice-edge-tts/service/Dockerfile
git commit -m "build(voice-edge-tts): Linux ARM64/AMD64 Dockerfile"
```

---

### Task A10: README

**Files:**
- Create: `extensions/voice-edge-tts/README.md`

- [ ] **Step 1: Write README**

```markdown
# voice-edge-tts

Cross-platform CPU TTS extension for NeoMind. Wraps `sherpa-onnx` ZipVoice
(zero-shot, ZH+EN) behind the same NDJSON `/tts/stream` contract as
`moss-tts-nano` and `cosyvoice-3`, so `voice-assistant` can switch backends
by changing one env var.

## Why

| | moss-tts-nano | cosyvoice-3 | **voice-edge-tts** |
|---|---|---|---|
| Mac CPU RTF | good | 2.5x (unusable) | **<0.5** |
| Linux ARM CPU | ✅ | ❌ CUDA-only | ✅ |
| ZH quality | Fair | Excellent | Good |
| Clone | Yes (MOSS) | Yes (zero-shot) | Yes (zero-shot) |
| Footprint | ~200MB | ~1GB | ~150MB |

## Quickstart

```bash
cd extensions/voice-edge-tts/service
pip install -r requirements.txt
./start.sh
# First start downloads ~150MB ZipVoice model to ~/.cache/sherpa-onnx
curl http://127.0.0.1:9386/health
```

## Endpoints

- `POST /tts` → full WAV bytes
- `POST /tts/stream` → NDJSON, one line per PCM chunk: `{seq, data, sample_rate, channels, is_pause}`
- `GET /voices` → `{"voices": ["中文女"]}`
- `GET /health`

## Switch voice-assistant to use it

In `extensions/voice-assistant/service/start.sh`:
```bash
export VOICE_ASSISTANT_TTS_URL=http://127.0.0.1:9386
```

## Environment

| Var | Default | Purpose |
|-----|---------|---------|
| `VOICE_EDGE_TTS_HOST` | 127.0.0.1 | Bind host |
| `VOICE_EDGE_TTS_PORT` | 9386 | Bind port |
| `VOICE_EDGE_TTS_CPU_THREADS` | 2 | sherpa-onnx inference threads |
| `VOICE_EDGE_TTS_MODEL_DIR` | ~/.cache/sherpa-onnx | Model cache root |

## Bundle a custom voice

Replace `service/assets/default_prompt.wav` and `.txt`. The transcript MUST
match the audio for ZipVoice zero-shot to work. Recommended: 5-10s clean
16kHz mono clip.
```

- [ ] **Step 2: Commit**

```bash
git add extensions/voice-edge-tts/README.md
git commit -m "docs(voice-edge-tts): README with quickstart, endpoints, env vars"
```

---

## Part B — Silero VAD in voice-assistant

### Task B1: sherpa-onnx VAD loader

**Files:**
- Modify: `extensions/voice-assistant/service/server.py` (add `_SILERO_VAD_SINGLETON` block + helper).

- [ ] **Step 1: Add Silero config block near the existing FSMN config (around L76)**

After the existing `FSMN_VAD_SCHEDULE` block, insert:

```python
# Silero VAD via sherpa-onnx — preferred backend.
# License: MIT. 1.7MB. <1ms/frame.
SILERO_VAD_MODEL_PATH = os.environ.get(
    "SILERO_VAD_MODEL_PATH",
    str(Path.home() / ".cache" / "sherpa-onnx" / "silero_vad.onnx"),
)
SILERO_VAD_THRESHOLD = float(os.environ.get("SILERO_VAD_THRESHOLD", "0.5"))
SILERO_VAD_MIN_SPEECH_MS = int(os.environ.get("SILERO_VAD_MIN_SPEECH_MS", "250"))
SILERO_VAD_SILENCE_MS = int(os.environ.get("SILERO_VAD_SILENCE_MS", "500"))
```

Add `from pathlib import Path` to imports at top of file if not present.

- [ ] **Step 2: Add the singleton loader block**

After the existing FSMN loader block (around L511), insert:

```python
# ---------------------------------------------------------------------------
# Silero VAD config — loaded once at startup if backend=silero.
# sherpa-onnx VoiceActivityDetector is stateful (owns its ring buffer), so
# we cache the VadModelConfig here and each VoiceSession creates its own
# VoiceActivityDetector from it in __init__.
# ---------------------------------------------------------------------------
_SILERO_VAD_CONFIG = None
if VAD_BACKEND == "silero":
    try:
        import sherpa_onnx
        from pathlib import Path

        silero_path = Path(SILERO_VAD_MODEL_PATH)
        if not silero_path.is_file():
            silero_path.parent.mkdir(parents=True, exist_ok=True)
            import urllib.request
            url = "https://github.com/snakers4/silero-vad/raw/master/files/silero_vad.onnx"
            logger.info("Downloading Silero VAD model → %s", silero_path)
            urllib.request.urlretrieve(url, silero_path)

        _SILERO_VAD_CONFIG = sherpa_onnx.VadModelConfig()
        _SILERO_VAD_CONFIG.silero_vad.model = str(silero_path)
        _SILERO_VAD_CONFIG.silero_vad.threshold = SILERO_VAD_THRESHOLD
        _SILERO_VAD_CONFIG.sample_rate = SAMPLE_RATE
        _SILERO_VAD_CONFIG.provider = "cpu"
        if not _SILERO_VAD_CONFIG.validate():
            raise RuntimeError("Silero VAD config invalid")
        logger.info("Silero VAD config ready: %s", silero_path)
    except Exception as e:
        logger.warning("Silero VAD load failed, falling back to energy: %s", e)
        _SILERO_VAD_CONFIG = None
        VAD_BACKEND = "energy"
```

- [ ] **Step 3: Update `VoiceSession.__init__` to instantiate a per-session VAD**

In `VoiceSession.__init__` (around L529), add:

```python
# Silero VAD state
self._silero_vad = None
self._silero_speech_buffer: list[np.ndarray] = []
self._silero_in_speech = False
if VAD_BACKEND == "silero" and _SILERO_VAD_CONFIG is not None:
    try:
        import sherpa_onnx
        self._silero_vad = sherpa_onnx.VoiceActivityDetector(
            _SILERO_VAD_CONFIG, buffer_size_in_seconds=30,
        )
    except Exception as e:
        logger.warning("per-session Silero VAD init failed: %s", e)
        self._silero_vad = None
```

- [ ] **Step 4: Commit**

```bash
git add extensions/voice-assistant/service/server.py
git commit -m "feat(voice-assistant): add Silero VAD config + per-session detector"
```

---

### Task B2: `VoiceSession._feed_pcm_silero`

**Files:**
- Modify: `extensions/voice-assistant/service/server.py` — add method + dispatch.

- [ ] **Step 1: Add the method**

After the `_feed_pcm_fsmn` method and its helpers (around L719, after `_finalize_fsmn`), insert:

```python
def _feed_pcm_silero(self, samples_int16: np.ndarray) -> Optional[np.ndarray]:
    """Silero neural VAD via sherpa-onnx.

    Silero emits segments via accept_waveform + (empty()? front : pop) protocol.
    We accumulate speech samples until the model signals end-of-utterance.
    """
    if self._silero_vad is None:
        return self._feed_pcm_energy(samples_int16)

    audio_f32 = samples_int16.astype(np.float32) / 32768.0

    # AEC echo window: while TTS playback is active, skip feeding the VAD
    # entirely so it can't misclassify TTS leak as user speech.
    if self._aec_active_now():
        return None

    self._silero_vad.accept_waveform(SAMPLE_RATE, audio_f32)

    while not self._silero_vad.empty():
        segment = self._silero_vad.front
        self._silero_vad.pop()
        samples = np.asarray(segment.samples, dtype=np.float32)
        # Silero emits a complete segment (start→end). Return it for ASR.
        self._silero_in_speech = False
        pcm_bytes = (samples * 32767.0).astype("<i2").tobytes()
        return pcm_bytes
    return None
```

- [ ] **Step 2: Update the `feed_pcm` dispatcher**

At L580, change:

```python
def feed_pcm(self, samples_int16: np.ndarray) -> Optional[np.ndarray]:
    """..."""
    if self._fsmn_vad is not None:
        return self._feed_pcm_fsmn(samples_int16)
    return self._feed_pcm_energy(samples_int16)
```

to:

```python
def feed_pcm(self, samples_int16: np.ndarray) -> Optional[np.ndarray]:
    """..."""
    if self._silero_vad is not None:
        return self._feed_pcm_silero(samples_int16)
    if self._fsmn_vad is not None:
        return self._feed_pcm_fsmn(samples_int16)
    return self._feed_pcm_energy(samples_int16)
```

- [ ] **Step 3: Commit**

```bash
git add extensions/voice-assistant/service/server.py
git commit -m "feat(voice-assistant): _feed_pcm_silero backend + dispatch"
```

---

### Task B3: Default switch + requirements

**Files:**
- Modify: `extensions/voice-assistant/service/requirements.txt`
- Modify: `extensions/voice-assistant/service/start.sh`

- [ ] **Step 1: Add sherpa-onnx to requirements**

Append to `extensions/voice-assistant/service/requirements.txt`:

```
sherpa-onnx>=1.10
```

- [ ] **Step 2: Update `start.sh` defaults**

In `extensions/voice-assistant/service/start.sh`, the existing line `export MOSS_TTS_URL="${MOSS_TTS_URL:-http://127.0.0.1:9385}"` must be **replaced** (not supplemented) with the new env var, otherwise it remains as a confusing fallback. Replace that line with:

```bash
export VOICE_ASSISTANT_VAD_BACKEND="${VOICE_ASSISTANT_VAD_BACKEND:-silero}"
export VOICE_ASSISTANT_TTS_URL="${VOICE_ASSISTANT_TTS_URL:-http://127.0.0.1:9386}"
export VOICE_ASSISTANT_VOICE="${VOICE_ASSISTANT_VOICE:-中文女}"
```

The server.py resolution order at L60-63 is `VOICE_ASSISTANT_TTS_URL or MOSS_TTS_URL or default`, so once `VOICE_ASSISTANT_TTS_URL` is set, the old `MOSS_TTS_URL` line is dead. Delete the `MOSS_TTS_URL` line to avoid confusion. To keep cosyvoice-3 as an opt-in fallback, operators set `VOICE_ASSISTANT_TTS_URL=http://127.0.0.1:9385` explicitly at run time.

- [ ] **Step 3: Install the new dep**

```bash
cd extensions/voice-assistant/service
pip install -r requirements.txt
```

- [ ] **Step 4: Commit**

```bash
git add extensions/voice-assistant/service/requirements.txt extensions/voice-assistant/service/start.sh
git commit -m "feat(voice-assistant): default VAD=silero, TTS=voice-edge-tts:9386"
```

---

### Task B4: Silero VAD unit + integration test

- [ ] **Step 1: Write a smoke test for the VAD**

Create `extensions/voice-assistant/service/test_silero_vad.py`:

```python
"""Smoke test: Silero VAD should fire on a synthetic speech-like burst
and stay silent on pure silence."""
import numpy as np
import pytest


@pytest.fixture
def session(monkeypatch):
    import server as srv
    monkeypatch.setattr(srv, "VAD_BACKEND", "silero")
    # Force config-load path.
    monkeypatch.setattr(srv, "SAMPLE_RATE", 16000)
    if srv._SILERO_VAD_CONFIG is None:
        pytest.skip("Silero unavailable")
    sess = srv.VoiceSession.__new__(srv.VoiceSession)
    sess._silero_vad = None
    sess._aec_active_now = lambda: False
    srv.VoiceSession.__init__(sess, ws=None, session_id="t")
    return sess


def test_silence_no_speech(session):
    sil = np.zeros(16000, dtype=np.int16)  # 1s silence
    for i in range(0, len(sil), 512):
        result = session.feed_pcm(sil[i:i+512])
    assert result is None


def test_burst_detects_speech(session):
    # Use a real speech-like signal: short impulse train mimicking voiced speech.
    # Pure 440Hz sine rarely triggers Silero v5 (trained on real speech).
    sr = 16000
    t = np.arange(sr) / sr  # 1 second
    # 120Hz glottal source (typical male pitch) amplitude-modulated
    voiced = 0.4 * np.sin(2 * np.pi * 120 * t) * (0.5 + 0.5 * np.sin(2 * np.pi * 3 * t))
    burst = (voiced * 32767).astype(np.int16)
    saw_speech = False
    for i in range(0, len(burst), 512):
        result = session.feed_pcm(burst[i:i+512])
        if result is not None:
            saw_speech = True
    assert saw_speech, "Silero failed to detect a voiced burst — check model load"
```

- [ ] **Step 2: Run**

```bash
cd extensions/voice-assistant/service
python -m pytest test_silero_vad.py -v
```

- [ ] **Step 3: Manual end-to-end (requires mic + speaker)**

```bash
# Terminal 1
cd extensions/voice-edge-tts/service && ./start.sh
# Terminal 2
cd extensions/sensevoice-asr/service && ./start.sh
# Terminal 3
cd extensions/voice-assistant/service
VOICE_ASSISTANT_VAD_BACKEND=silero ./start.sh
# Terminal 4 — open poc.html, speak a short phrase, observe ASR + TTS round-trip.
```

Expected:
- Endpoint detection fires within 500ms of speaking stop
- No false triggers during TTS playback (AEC echo-window suppresses)
- ASR captures complete phrase

- [ ] **Step 4: Commit**

```bash
git add extensions/voice-assistant/service/test_silero_vad.py
git commit -m "test(voice-assistant): Silero VAD smoke tests"
```

---

## Part C — Integration verification

### Task C1: voice-assistant end-to-end with new stack

- [ ] **Step 1: Bring up all three services**

```bash
# Terminal 1 — TTS
cd extensions/voice-edge-tts/service && ./start.sh
# Terminal 2 — ASR
cd extensions/sensevoice-asr/service && ./start.sh
# Terminal 3 — orchestrator with new defaults
cd extensions/voice-assistant/service && ./start.sh
```

- [ ] **Step 2: Confirm orchestrator picked up new backends**

voice-assistant is a WebSocket server (no HTTP routes). Check the startup log lines instead — server.py at L1043 already prints `host, port, ASR_URL, TTS_URL, TTS_VOICE` on boot:

```bash
# Tail the orchestrator's log and verify:
grep -E "(TTS_URL|VAD_BACKEND|tts_url|9386|silero)" /tmp/voice-assistant.log || \
    journalctl -u voice-assistant --since "1 min ago" | grep -E "(TTS|VAD|9386|silero)"
# Expected: a startup line showing tts_url=http://127.0.0.1:9386
```

- [ ] **Step 3: Run the existing latency measurement script**

```bash
cd extensions/voice-assistant/service
python measure_first_sentence.py --n 3
```

Expected: first-sentence latency dramatically better than CosyVoice 3 CPU numbers (target < 1s for TTS).

- [ ] **Step 4: Record results in README**

Append to `extensions/voice-edge-tts/README.md`:

```markdown
## End-to-end (voice-assistant + sensevoice + voice-edge-tts + Silero VAD)

| Metric | CosyVoice 3 (CPU) | voice-edge-tts |
|--------|-------------------|----------------|
| First-sentence TTS | ~8-10s | TBD |
| RTF | 2.5x | TBD |
```

- [ ] **Step 5: Commit**

```bash
git add extensions/voice-edge-tts/README.md
git commit -m "docs: record end-to-end latency with new edge stack"
```

---

## Risk Register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| sherpa-onnx `OfflineTts.generate()` signature for ZipVoice differs from plan | **High** (fresh model) | Task A4 Step 3 verifies the real shape before wiring up `/tts`. Update `_generate()` in A5 to match. |
| Silero VAD segments too short → ASR gets fragments | Medium | Set `min_silence_duration_ms=500` via config; tune in B2 if needed. |
| Mac M4 RTF > 0.5 despite smaller model | Low | A8 benchmark catches this; fallback = keep cosyvoice-3 for dev, voice-edge-tts for prod only. |
| Default `default_prompt.wav` transcript mismatch | Medium | A6 Step 1 explicitly sets transcript; ZipVoice will fail audibly if mismatched — easy to catch. |
| Dockerfile build fails on arm64 from Mac | Medium | A9 Step 2 has native-platform fallback; arm64 verified on real target hardware later. |

## Out of scope (Phase 2)

- Wake word detection (sherpa KWS) — NeoMind main system triggers via events.
- AEC hardware integration (speexdsp) — assumes close-mic / headset for now.
- Bi-streaming token-level TTS — would need voice-assistant producer/consumer rewrite.
- Deleting moss-tts-nano / cosyvoice-3 — kept as CUDA fallback / historical reference.
- Publishing to marketplace (index.json, metadata.json) — post-PoC.
