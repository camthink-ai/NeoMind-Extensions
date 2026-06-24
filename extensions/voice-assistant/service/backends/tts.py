"""TTS backend implementations."""
from __future__ import annotations

import base64
import json
import logging
from typing import AsyncIterator

import httpx

from contracts import TtsChunk

logger = logging.getLogger("voice-assistant.tts")


class ZipVoiceHTTP:
    """ZipVoice TTS via voice-edge-tts HTTP service (port 9386).
    NDJSON /tts/stream contract — one line per PCM chunk.

    Implements the TTSBackend Protocol from contracts.py.
    """

    def __init__(self, url: str, voice: str = "中文女", timeout: float = 60.0):
        self.url = url
        self.voice = voice
        self.timeout = timeout

    async def synthesize(self, text: str, voice: str) -> bytes:
        """Batch: synthesize full utterance. Concatenates all NDJSON PCM chunks
        into one int16 LE PCM bytes blob.
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
                    pcm_chunks.append(base64.b64decode(obj["data"]))
                return b"".join(pcm_chunks)

    async def stream(self, text: str, voice: str) -> AsyncIterator[TtsChunk]:
        """Streaming variant. Yields one TtsChunk per NDJSON line.

        Phase 2 path — wired into the orchestrator in Task 14.
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
                    yield TtsChunk(
                        pcm_int16=base64.b64decode(obj["data"]),
                        sample_rate=int(obj.get("sample_rate", 24000)),
                        is_final=False,
                    )
