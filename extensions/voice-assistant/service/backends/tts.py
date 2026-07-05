"""TTS backend implementations."""
from __future__ import annotations

import asyncio
import base64
import json
import logging
import os
from pathlib import Path
from typing import AsyncIterator

import httpx
import numpy as np

from contracts import TtsChunk

logger = logging.getLogger("voice-assistant.tts")


# Module-level cache for the heavy sherpa_onnx import (see backends/asr.py
# for rationale). Resolved via _get_sherpa() so tests can monkeypatch.
_sherpa = None


def _get_sherpa():
    """Lazy sherpa_onnx import, cached on first call."""
    global _sherpa
    if _sherpa is None:
        import sherpa_onnx  # noqa: F401
        _sherpa = sherpa_onnx
    return _sherpa


def _to_mono(pcm_int16: bytes, channels: int) -> bytes:
    """Downmix int16 LE interleaved PCM to mono. No-op if already mono."""
    if channels <= 1:
        return pcm_int16
    arr = np.frombuffer(pcm_int16, dtype="<i2").astype(np.float32)
    if arr.size % channels != 0:
        # Truncate to a whole-frame boundary.
        arr = arr[: arr.size - (arr.size % channels)]
    return arr.reshape(-1, channels).mean(axis=1).astype("<i2").tobytes()


class ZipVoiceHTTP:
    """NDJSON /tts/stream TTS client.

    Shared by zipvoice_http and moss_tts_http — both speak the same
    contract. TTS may emit mono or stereo; we downmix to mono here so
    the rest of the pipeline can assume single-channel.

    Implements the TTSBackend Protocol from contracts.py.
    """

    def __init__(self, url: str, voice: str = "中文女", timeout: float = 60.0):
        self.url = url
        self.voice = voice
        self.timeout = timeout

    async def synthesize(self, text: str, voice: str) -> bytes:
        """Batch: synthesize full utterance. Concatenates all NDJSON PCM chunks
        into one int16 LE PCM mono bytes blob.
        """
        async with httpx.AsyncClient(timeout=self.timeout) as client:
            async with client.stream(
                "POST",
                f"{self.url}/tts/stream",
                json={"text": text, "voice": voice or self.voice},
            ) as resp:
                resp.raise_for_status()
                pcm_chunks: list[bytes] = []
                async for line in resp.aiter_lines():
                    line = line.strip() if isinstance(line, str) else line.decode().strip()
                    if not line:
                        continue
                    obj = json.loads(line)
                    if "error" in obj:
                        raise RuntimeError(obj["error"])
                    ch = int(obj.get("channels", 1))
                    pcm_chunks.append(_to_mono(base64.b64decode(obj["data"]), ch))
                return b"".join(pcm_chunks)

    async def stream(self, text: str, voice: str) -> AsyncIterator[TtsChunk]:
        """Streaming variant. Yields one TtsChunk per NDJSON line.

        Downmixes to mono at the source so downstream can assume 1ch.
        """
        async with httpx.AsyncClient(timeout=self.timeout) as client:
            async with client.stream(
                "POST",
                f"{self.url}/tts/stream",
                json={"text": text, "voice": voice or self.voice},
            ) as resp:
                resp.raise_for_status()
                async for line in resp.aiter_lines():
                    line = line.strip() if isinstance(line, str) else line.decode().strip()
                    if not line:
                        continue
                    obj = json.loads(line)
                    if "error" in obj:
                        raise RuntimeError(obj["error"])
                    ch = int(obj.get("channels", 1))
                    yield TtsChunk(
                        pcm_int16=_to_mono(base64.b64decode(obj["data"]), ch),
                        sample_rate=int(obj.get("sample_rate", 24000)),
                        is_final=False,
                    )


# ---------------------------------------------------------------------------
# In-process ZipVoice backend (all-in-one mode)
# ---------------------------------------------------------------------------
# Calls sherpa_onnx.OfflineTts directly — no HTTP boundary. ZipVoice is
# batch-only (no true streaming), so stream() yields ONE TtsChunk with
# is_final=True. Implements TTSBackend Protocol from contracts.py.

ZIPVOICE_MODEL_NAME = "sherpa-onnx-zipvoice-distill-int8-zh-en-emilia"
ZIPVOICE_VOCODER_NAME = "vocos_24khz.onnx"


def _download_zipvoice_model(dest: Path) -> None:
    """Download + extract the ZipVoice model tar.bz2 into dest's parent."""
    import tarfile
    import urllib.request

    url = (
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/"
        f"tts-models/{ZIPVOICE_MODEL_NAME}.tar.bz2"
    )
    tmp = dest.parent / f"{ZIPVOICE_MODEL_NAME}.tar.bz2"
    logger.info("Downloading ZipVoice model from %s -> %s", url, tmp)
    urllib.request.urlretrieve(url, tmp)
    logger.info("Extracting %s", tmp)
    with tarfile.open(tmp, "r:bz2") as t:
        t.extractall(dest.parent)
    tmp.unlink(missing_ok=True)
    if not (dest / "encoder.int8.onnx").exists():
        raise RuntimeError(f"encoder.int8.onnx missing after extract at {dest}")


def _download_vocoder(dest: Path) -> None:
    """Download the vocos_24khz.onnx vocoder."""
    import urllib.request

    url = (
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/"
        "vocoder-models/" + ZIPVOICE_VOCODER_NAME
    )
    logger.info("Downloading vocoder -> %s", dest)
    urllib.request.urlretrieve(url, dest)


class ZipVoiceInprocTTS:
    """In-process ZipVoice TTS. No HTTP boundary.

    Loads OfflineTts eagerly at construction. Auto-downloads model +
    vocoder on first construction if missing. Loads default prompt audio
    from `service/assets/default_prompt.{wav,txt}` (override via kwargs).
    `synthesize()` / `stream()` wrap sync `tts.generate()` in
    `asyncio.to_thread()` to keep the event loop responsive.

    ZipVoice outputs 24 kHz mono; browser-side resampling handles the rest.
    """

    def __init__(
        self,
        voice: str = "中文女",
        num_threads: int = 2,
        model_dir: str | None = None,
        prompt_wav: str | None = None,
        prompt_text: str | None = None,
    ):
        sherpa = _get_sherpa()
        self.voice = voice

        base = model_dir or os.environ.get(
            "VOICE_EDGE_TTS_MODEL_DIR",
            str(Path.home() / ".cache" / "sherpa-onnx"),
        )
        d = Path(base) / ZIPVOICE_MODEL_NAME
        if not (d / "encoder.int8.onnx").exists():
            d.mkdir(parents=True, exist_ok=True)
            _download_zipvoice_model(d)
        vocoder_path = Path(base) / ZIPVOICE_VOCODER_NAME
        if not vocoder_path.exists():
            _download_vocoder(vocoder_path)

        logger.info(
            "Loading ZipVoice OfflineTts (model=%s, threads=%d)", d, num_threads,
        )
        cfg = sherpa.OfflineTtsConfig(
            model=sherpa.OfflineTtsModelConfig(
                zipvoice=sherpa.OfflineTtsZipvoiceModelConfig(
                    encoder=str(d / "encoder.int8.onnx"),
                    decoder=str(d / "decoder.int8.onnx"),
                    vocoder=str(vocoder_path),
                    tokens=str(d / "tokens.txt"),
                    lexicon=str(d / "lexicon.txt"),
                    data_dir=str(d / "espeak-ng-data"),
                ),
                num_threads=num_threads,
                debug=False,
                provider="cpu",
            ),
            max_num_sentences=2,
        )
        if not cfg.validate():
            raise RuntimeError("sherpa-onnx ZipVoice config invalid; check paths")
        self._tts = sherpa.OfflineTts(cfg)
        self.sample_rate_out = 24000
        logger.info("ZipVoice OfflineTts ready (sr=%d)", self.sample_rate_out)

        # Default prompt audio — fallback for any voice ID. Callers can
        # override per-request via prompt_wav/prompt_text, but the
        # TTSBackend Protocol only passes `voice`, so we resolve at
        # construction time.
        default_wav = Path(__file__).parent.parent / "assets" / "default_prompt.wav"
        default_txt = Path(__file__).parent.parent / "assets" / "default_prompt.txt"
        self._prompt_wav = prompt_wav or str(default_wav)
        self._prompt_text = prompt_text
        if self._prompt_text is None:
            if default_txt.is_file():
                self._prompt_text = default_txt.read_text(encoding="utf-8").strip()
            else:
                raise RuntimeError(
                    f"default prompt text missing: {default_txt}. "
                    "Provide prompt_text= or bundle assets/default_prompt.txt."
                )
        self._prompt_samples, self._prompt_sr = self._load_prompt(self._prompt_wav)

    @staticmethod
    def _load_prompt(path: str) -> tuple[list[float], int]:
        """Load any audio as 16kHz mono float32 list (ZipVoice expects 16kHz prompt)."""
        import soundfile as sf

        data, sr = sf.read(path, dtype="float32", always_2d=False)
        if data.ndim > 1:
            data = data.mean(axis=1)
        if sr != 16000:
            n = int(len(data) * 16000 / sr)
            idx = np.linspace(0, len(data) - 1, n)
            data = np.interp(idx, np.arange(len(data)), data).astype(np.float32)
            sr = 16000
        return data.tolist(), int(sr)

    def _generate_sync(self, text: str) -> np.ndarray:
        """Run one generate() call. Returns float32 samples."""
        audio = self._tts.generate(
            text, self._prompt_text, self._prompt_samples, self._prompt_sr,
        )
        return np.asarray(audio.samples, dtype=np.float32).reshape(-1)

    async def synthesize(self, text: str, voice: str) -> bytes:
        """Batch: synthesize full utterance. float32 → int16 LE bytes."""
        audio = await asyncio.to_thread(self._generate_sync, text)
        pcm = np.clip(audio, -1.0, 1.0)
        return (pcm * 32767.0).astype("<i2").tobytes()

    async def stream(self, text: str, voice: str) -> AsyncIterator[TtsChunk]:
        """Streaming variant. ZipVoice is batch-only, so yields ONE chunk
        with is_final=True. The orchestrator's pipeline already handles
        the single-chunk case (same as the HTTP path with NDJSON=1 line).
        """
        pcm_bytes = await self.synthesize(text, voice)
        yield TtsChunk(
            pcm_int16=pcm_bytes,
            sample_rate=self.sample_rate_out,
            is_final=True,
        )

