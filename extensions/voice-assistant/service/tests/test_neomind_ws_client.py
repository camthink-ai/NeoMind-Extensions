"""NeoMindWSClient tests with mocked WebSocket."""
from __future__ import annotations

import asyncio
import json
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from backends.llm import NeoMindWSClient
from contracts import LlmEvent


@pytest.mark.asyncio
async def test_stream_emits_content_events():
    """NeoMind Content events → LlmEvent(type='Content')."""
    client = NeoMindWSClient(url="ws://mock:9375/api/chat", token="t")

    fake_events = [
        {"type": "Thinking"},
        {"type": "Content", "content": "你好"},
        {"type": "ToolCallStart", "toolName": "weather"},
        {"type": "ToolCallEnd"},
        {"type": "Content", "content": "世界"},
        {"type": "end", "sessionId": "s1"},
    ]
    mock_ws = AsyncMock()
    mock_ws.send = AsyncMock()
    mock_ws.closed = False
    mock_ws.__aiter__ = _make_aiter(fake_events)
    mock_ws.__aenter__ = AsyncMock(return_value=mock_ws)
    mock_ws.__aexit__ = AsyncMock(return_value=None)

    with patch("websockets.connect", return_value=mock_ws):
        events = []
        async for evt in client.stream("hi", session_id="s1"):
            events.append(evt)

    content = [e for e in events if e.type == "Content"]
    assert len(content) == 2
    assert content[0].text == "你好"
    assert content[1].text == "世界"
    assert events[-1].type == "end"


@pytest.mark.asyncio
async def test_stream_filters_interrupted_marker():
    """Content with '\\n\\n[Interrupted]' must NOT be emitted (post-cancel marker)."""
    client = NeoMindWSClient(url="ws://mock:9375/api/chat", token="t")

    fake_events = [
        {"type": "Content", "content": "real reply"},
        {"type": "Content", "content": "\n\n[Interrupted]"},
        {"type": "end"},
    ]
    mock_ws = AsyncMock()
    mock_ws.send = AsyncMock()
    mock_ws.closed = False
    mock_ws.__aiter__ = _make_aiter(fake_events)
    mock_ws.__aenter__ = AsyncMock(return_value=mock_ws)
    mock_ws.__aexit__ = AsyncMock(return_value=None)

    with patch("websockets.connect", return_value=mock_ws):
        events = []
        async for evt in client.stream("hi", session_id="s1"):
            events.append(evt)

    content_texts = [e.text for e in events if e.type == "Content"]
    assert "real reply" in content_texts
    assert "\n\n[Interrupted]" not in content_texts


@pytest.mark.asyncio
async def test_cancel_sends_underscore_cancel_message():
    client = NeoMindWSClient(url="ws://mock:9375/api/chat", token="t")
    mock_ws = AsyncMock()
    mock_ws.closed = False
    mock_ws.send = AsyncMock()
    client._active_ws = mock_ws  # inject active connection
    client._llm_completed = False

    await client.cancel("s1")
    mock_ws.send.assert_called_once_with(json.dumps({"type": "__CANCEL__"}))


@pytest.mark.asyncio
async def test_cancel_skipped_if_llm_completed():
    client = NeoMindWSClient(url="ws://mock:9375/api/chat", token="t")
    mock_ws = AsyncMock()
    mock_ws.closed = False
    mock_ws.send = AsyncMock()
    client._active_ws = mock_ws
    client._llm_completed = True  # LLM already ended

    await client.cancel("s1")
    mock_ws.send.assert_not_called()


def _make_aiter(events):
    async def aiter(self):
        for e in events:
            yield json.dumps(e)
    return aiter
