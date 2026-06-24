"""LLM backend implementations."""
from __future__ import annotations

import asyncio
import json
import logging
from typing import AsyncIterator

import httpx

from contracts import LlmEvent

logger = logging.getLogger("voice-assistant.llm")


class FakeLLMClient:
    """Echo LLM for testing. Implements LLMClient Protocol.

    Yields LlmEvent(type="Content") chunks then a final LlmEvent(type="end").
    Note: this yields raw token chunks, NOT sentence-split fragments.
    The orchestrator (Task 14) is responsible for sentence splitting.
    """

    def __init__(self, reply_template: str = "你刚才说: {text}"):
        self.reply_template = reply_template
        self._cancelled_sessions: set[str] = set()

    async def stream(self, user_text: str, session_id: str) -> AsyncIterator[LlmEvent]:
        if session_id in self._cancelled_sessions:
            yield LlmEvent(type="end")
            return
        reply = self.reply_template.format(text=user_text)
        # Emit Content in ~10-char chunks to simulate streaming tokens
        for i in range(0, len(reply), 10):
            if session_id in self._cancelled_sessions:
                yield LlmEvent(type="end")
                return
            chunk = reply[i:i + 10]
            yield LlmEvent(type="Content", text=chunk)
            await asyncio.sleep(0.01)
        yield LlmEvent(type="end")

    async def cancel(self, session_id: str) -> None:
        self._cancelled_sessions.add(session_id)


class OllamaHTTPClient:
    """Ollama local LLM via HTTP streaming (port 11434).

    Streams Content events (one per token) from Ollama's /api/chat NDJSON,
    then a final 'end' event. Implements the LLMClient Protocol.

    The orchestrator (Task 14) is responsible for sentence-splitting the
    Content events before forwarding to TTS.
    """

    def __init__(
        self,
        url: str = "http://127.0.0.1:11434",
        model: str = "qwen3:1.7b",
        system_prompt: str | None = None,
        timeout: float = 60.0,
    ):
        self.url = url
        self.model = model
        self.system_prompt = system_prompt or (
            "你是简洁的中文语音助手。用口语化短句回答，每句不超过 20 字。"
            "不要使用 Markdown 格式。"
        )
        self.timeout = timeout
        self._cancelled: set[str] = set()

    async def stream(self, user_text: str, session_id: str) -> AsyncIterator[LlmEvent]:
        """Stream Content events from Ollama, then a final 'end' event.

        Checks self._cancelled between tokens; if cancelled, emits 'end' and returns.
        """
        try:
            async with httpx.AsyncClient(timeout=self.timeout) as client:
                async with client.stream(
                    "POST",
                    f"{self.url}/api/chat",
                    json={
                        "model": self.model,
                        "messages": [
                            {"role": "system", "content": self.system_prompt},
                            {"role": "user", "content": user_text},
                        ],
                        "stream": True,
                    },
                ) as resp:
                    resp.raise_for_status()
                    async for line in resp.aiter_lines():
                        if session_id in self._cancelled:
                            yield LlmEvent(type="end")
                            return
                        if not line.strip():
                            continue
                        try:
                            obj = json.loads(line)
                        except Exception:
                            continue
                        if obj.get("done"):
                            yield LlmEvent(type="end")
                            return
                        chunk = obj.get("message", {}).get("content", "")
                        if chunk:
                            yield LlmEvent(type="Content", text=chunk)
        except httpx.HTTPError as e:
            yield LlmEvent(type="Content", text=f"(LLM 错误: {e})")
            yield LlmEvent(type="end")

    async def cancel(self, session_id: str) -> None:
        self._cancelled.add(session_id)
