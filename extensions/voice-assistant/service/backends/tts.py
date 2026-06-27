"""TTS backend implementations."""
from __future__ import annotations

import base64
import json
import logging
from typing import AsyncIterator

import httpx
import numpy as np

from contracts import TtsChunk

logger = logging.getLogger("voice-assistant.tts")


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

