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
    # Cancel uses ChatRequest schema: message == "__CANCEL__", with the
    # captured NeoMind session id preferred over the caller-supplied id.
    # Here no session was captured, so the caller's "s1" is used.
    mock_ws.send.assert_called_once_with(
        json.dumps({"message": "__CANCEL__", "sessionId": "s1"})
    )


@pytest.mark.asyncio
async def test_cancel_prefers_captured_neomind_session_id():
    """If a real NeoMind sessionId was captured from session_created, cancel
    must use it (not the caller-supplied str(id(pipeline)))."""
    client = NeoMindWSClient(url="ws://mock:9375/api/chat", token="t")
    client._neomind_session_id = "real-uuid-from-server"
    mock_ws = AsyncMock()
    mock_ws.closed = False
    mock_ws.send = AsyncMock()
    client._active_ws = mock_ws
    client._llm_completed = False

    await client.cancel("caller-supplied-id")
    mock_ws.send.assert_called_once_with(
        json.dumps({"message": "__CANCEL__", "sessionId": "real-uuid-from-server"})
    )


@pytest.mark.asyncio
async def test_stream_captures_session_id_from_session_created_event():
    """First-turn session_created event captures the server-assigned sessionId;
    second turn reuses it in the outbound payload."""
    client = NeoMindWSClient(url="ws://mock:9375/api/chat", token="t")

    fake_events = [
        {"type": "session_created", "sessionId": "server-uuid-123"},
        {"type": "Content", "content": "hi"},
        {"type": "end"},
    ]
    mock_ws = AsyncMock()
    mock_ws.send = AsyncMock()
    mock_ws.closed = False
    mock_ws.__aiter__ = _make_aiter(fake_events)
    mock_ws.__aenter__ = AsyncMock(return_value=mock_ws)
    mock_ws.__aexit__ = AsyncMock(return_value=None)

    sent_payloads: list[str] = []

    async def _record_send(payload):
        sent_payloads.append(payload)

    mock_ws.send.side_effect = _record_send

    with patch("websockets.connect", return_value=mock_ws):
        events = []
        async for evt in client.stream("hi", session_id="ignored"):
            events.append(evt)

    assert client._neomind_session_id == "server-uuid-123"
    # First turn: no captured id yet, so payload sessionId is None
    first_payload = json.loads(sent_payloads[0])
    assert first_payload["sessionId"] is None
    assert first_payload["message"] == "hi"


@pytest.mark.asyncio
async def test_first_turn_payload_includes_session_config_system_prompt():
    """PR2: voice_hint is no longer prepended to message via pageContext
    (which polluted every user turn). Instead it flows once, at session
    creation, as sessionConfig.systemPrompt. The platform reads this
    only when creating a new session — see ChatRequest.sessionConfig
    / handlers/sessions.rs auto-create branch.
    """
    client = NeoMindWSClient(
        url="ws://mock:9375/api/chat",
        token="t",
        voice_hint="[MODE] be brief",
    )
    mock_ws = AsyncMock()
    mock_ws.__aenter__ = AsyncMock(return_value=mock_ws)
    mock_ws.__aexit__ = AsyncMock(return_value=None)

    sent_payloads: list[str] = []

    async def _record_send(payload):
        sent_payloads.append(payload)

    mock_ws.send.side_effect = _record_send

    async def _mock_iter(_):
        yield '{"type": "session_created", "sessionId": "srv-1"}'
        yield '{"type": "Content", "content": "hi"}'
        yield '{"type": "end"}'

    mock_ws.__aiter__ = lambda self: _mock_iter(self)

    with patch("websockets.connect", return_value=mock_ws):
        async for _ in client.stream("hello", session_id="ignored"):
            pass

    first_payload = json.loads(sent_payloads[0])
    # User message verbatim — no hint prepended.
    assert first_payload["message"] == "hello"
    # sessionConfig carries the hint as systemPrompt.
    assert first_payload["sessionConfig"] == {"systemPrompt": "[MODE] be brief"}


@pytest.mark.asyncio
async def test_second_turn_payload_omits_session_config():
    """Second turn (sessionId already captured) must NOT send sessionConfig.
    The platform ignores it for existing sessions anyway, but re-sending
    clutters traces and wastes bandwidth.
    """
    client = NeoMindWSClient(
        url="ws://mock:9375/api/chat",
        token="t",
        voice_hint="[MODE] be brief",
    )
    client._neomind_session_id = "real-nm-sid"  # already captured

    mock_ws = AsyncMock()
    mock_ws.__aenter__ = AsyncMock(return_value=mock_ws)
    mock_ws.__aexit__ = AsyncMock(return_value=None)

    sent_payloads: list[str] = []

    async def _record_send(payload):
        sent_payloads.append(payload)

    mock_ws.send.side_effect = _record_send

    async def _mock_iter(_):
        yield '{"type": "Content", "content": "ok"}'
        yield '{"type": "end"}'

    mock_ws.__aiter__ = lambda self: _mock_iter(self)

    with patch("websockets.connect", return_value=mock_ws):
        async for _ in client.stream("again", session_id="ignored"):
            pass

    payload = json.loads(sent_payloads[0])
    assert payload["message"] == "again"
    assert payload["sessionId"] == "real-nm-sid"
    assert "sessionConfig" not in payload


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
