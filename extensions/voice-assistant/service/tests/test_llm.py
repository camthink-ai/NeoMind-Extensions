"""LLM backend tests."""
from __future__ import annotations

import pytest

from backends.llm import FakeLLMClient, OllamaHTTPClient
from contracts import LlmEvent


@pytest.mark.asyncio
async def test_fake_llm_emits_content_events():
    llm = FakeLLMClient(reply_template="Echo: {text}")
    events = []
    async for evt in llm.stream("hello", session_id="s1"):
        events.append(evt)
    # Should have at least one Content event and an end event
    content_events = [e for e in events if e.type == "Content"]
    assert len(content_events) >= 1
    # Check that "hello" appears in the combined content
    combined_text = "".join(e.text for e in content_events if e.text)
    assert "hello" in combined_text
    assert events[-1].type in ("end", "End")


@pytest.mark.asyncio
async def test_fake_llm_cancel_is_idempotent():
    llm = FakeLLMClient()
    await llm.cancel("s1")  # should not raise
    await llm.cancel("s1")  # second call also safe


@pytest.mark.asyncio
async def test_ollama_client_initializes():
    # Just verify construction doesn't fail
    llm = OllamaHTTPClient(url="http://mock:11434", model="qwen3:1.7b")
    assert llm.model == "qwen3:1.7b"
