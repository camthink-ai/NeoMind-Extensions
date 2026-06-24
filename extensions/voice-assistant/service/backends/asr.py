"""ASR backend implementations."""
from __future__ import annotations

import base64
import io
import logging
import wave

import httpx
import numpy as np

logger = logging.getLogger("voice-assistant.asr")


def _pcm_to_wav(pcm_int16: bytes, sample_rate: int, channels: int = 1) -> bytes:
    """Wrap raw int16 LE PCM in a WAV header."""
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(sample_rate)
        w.writeframes(pcm_int16)
    return buf.getvalue()


class SenseVoiceHTTPASR:
    """SenseVoice-Small ASR via HTTP (sensevoice-asr service on port 9383).

    Implements the ASRBackend Protocol from contracts.py.
    """

    def __init__(self, url: str, language: str = "auto", timeout: float = 30.0):
        self.url = url
        self.language = language
        self.timeout = timeout

    async def transcribe(self, pcm_float32: list[float], sample_rate: int) -> str:
        """Transcribe a complete audio segment. Returns the recognized text.

        Request shape matches the sensevoice-asr service:
        POST /asr with JSON {"audio_base64": <b64 wav>, "language": ..., "use_itn": true}.
        """
        pcm_int16 = (np.clip(np.asarray(pcm_float32, dtype=np.float32), -1.0, 1.0)
                     * 32767).astype("<i2")
        wav_bytes = _pcm_to_wav(pcm_int16.tobytes(), sample_rate)
        b64 = base64.b64encode(wav_bytes).decode()
        async with httpx.AsyncClient(timeout=self.timeout) as client:
            resp = await client.post(
                f"{self.url}/asr",
                json={
                    "audio_base64": b64,
                    "language": self.language,
                    "use_itn": True,
                },
            )
            resp.raise_for_status()
            return (resp.json().get("text") or "").strip()
