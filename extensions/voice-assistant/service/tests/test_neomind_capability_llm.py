"""NeoMindCapabilityLLM tests — capability-path backend that consumes
chat_chunk frames demultiplexed from the main /ws receive loop.

These tests verify the Python side of the ChatStream capability bridge
without requiring a live NeoMind platform or Rust extension. They use a
fake WebSocket and an asyncio.Queue mirroring how server.ws_handler feeds
the demultiplexed chat_chunk / chat_stream_started / chat_stream_end /
chat_stream_error frames into the backend.
"""
from __future__ import annotations

import asyncio
import json
from unittest.mock import AsyncMock

import pytest

from backends.llm import NeoMindCapabilityLLM
from contracts import LlmEvent


def _chunk(ctype: str, **extra) -> dict:
    """Build a chat_chunk frame wrapping an AgentEvent of type `ctype`."""
    chunk = {"type": ctype}
    chunk.update(extra)
    return {"type": "chat_chunk", "session_id": "n1", "chunk": chunk}


@pytest.mark.asyncio
async def test_stream_emits_content_and_end():
    """Content chunks → LlmEvent(type='Content'), terminal End → end."""
    ws = AsyncMock()
    ws.send_text = AsyncMock()
    chat_rx: asyncio.Queue = asyncio.Queue()
    client = NeoMindCapabilityLLM(ws=ws, chat_rx=chat_rx, voice_hint="")

    # Simulate the Rust extension pushing frames
    await chat_rx.put({"type": "chat_stream_started", "session_id": "n1"})
    await chat_rx.put(_chunk("Thinking"))
    await chat_rx.put(_chunk("Content", content="你好"))
    await chat_rx.put(_chunk("ToolCallStart", toolName="weather"))
    await chat_rx.put(_chunk("ToolCallEnd"))
    await chat_rx.put(_chunk("Content", content="世界"))
    await chat_rx.put(_chunk("end"))

    events = []
    async for evt in client.stream("hi", session_id="ignored"):
        events.append(evt)

    types = [e.type for e in events]
    assert "Thinking" in types
    assert types.count("Content") == 2
    content_texts = [e.text for e in events if e.type == "Content"]
    assert content_texts == ["你好", "世界"]
    tool_start = next(e for e in events if e.type == "ToolCallStart")
    assert tool_start.tool_name == "weather"
    assert events[-1].type == "end"

    # The request must have been sent with type=chat_stream_request and no
    # session_id on the first turn (host mints one).
    sent = json.loads(ws.send_text.call_args.args[0])
    assert sent["type"] == "chat_stream_request"
    assert sent["message"] == "hi"
    assert "session_id" not in sent
    # Captured session_id for multi-turn reuse
    assert client._neomind_session_id == "n1"


@pytest.mark.asyncio
async def test_stream_reuses_captured_session_id_on_second_turn():
    """After chat_stream_started captures a session_id, the next stream()
    call sends it back so the LLM keeps full history."""
    ws = AsyncMock()
    ws.send_text = AsyncMock()
    chat_rx: asyncio.Queue = asyncio.Queue()
    client = NeoMindCapabilityLLM(ws=ws, chat_rx=chat_rx, voice_hint="")
    client._neomind_session_id = "n-prev"

    await chat_rx.put(_chunk("Content", content="ok"))
    await chat_rx.put(_chunk("end"))

    async for _ in client.stream("second", session_id="ignored"):
        pass

    sent = json.loads(ws.send_text.call_args.args[0])
    assert sent["session_id"] == "n-prev"
    assert sent["message"] == "second"


@pytest.mark.asyncio
async def test_stream_handles_chat_stream_end_sentinel():
    """Rust emits a chat_stream_end sentinel after the End chunk. If only
    the sentinel arrives (no End inside a chunk), we must still terminate."""
    ws = AsyncMock()
    ws.send_text = AsyncMock()
    chat_rx: asyncio.Queue = asyncio.Queue()
    client = NeoMindCapabilityLLM(ws=ws, chat_rx=chat_rx, voice_hint="")

    await chat_rx.put({"type": "chat_stream_started", "session_id": "n2"})
    await chat_rx.put(_chunk("Content", content="partial"))
    await chat_rx.put({"type": "chat_stream_end", "session_id": "n2"})

    events = []
    async for evt in client.stream("hi", session_id="x"):
        events.append(evt)

    assert events[-1].type == "end"
    content = [e for e in events if e.type == "Content"]
    assert len(content) == 1


@pytest.mark.asyncio
async def test_stream_propagates_chat_stream_error():
    """Capability errors (e.g. host has no SessionManager yet) must surface
    as LlmEvent(type='Error') and terminate the iterator."""
    ws = AsyncMock()
    ws.send_text = AsyncMock()
    chat_rx: asyncio.Queue = asyncio.Queue()
    client = NeoMindCapabilityLLM(ws=ws, chat_rx=chat_rx, voice_hint="")

    await chat_rx.put({"type": "chat_stream_error", "error": "SessionManager unavailable"})

    events = []
    async for evt in client.stream("hi", session_id="x"):
        events.append(evt)

    assert any(e.type == "Error" and "SessionManager" in (e.text or "") for e in events)


@pytest.mark.asyncio
async def test_stream_filters_interrupted_marker():
    """Content with the post-cancel '\\n\\n[Interrupted]' marker must be
    filtered (same behavior as NeoMindWSClient)."""
    ws = AsyncMock()
    ws.send_text = AsyncMock()
    chat_rx: asyncio.Queue = asyncio.Queue()
    client = NeoMindCapabilityLLM(ws=ws, chat_rx=chat_rx, voice_hint="")

    await chat_rx.put(_chunk("Content", content="real reply"))
    await chat_rx.put(_chunk("Content", content="\n\n[Interrupted]"))
    await chat_rx.put(_chunk("end"))

    events = []
    async for evt in client.stream("hi", session_id="x"):
        events.append(evt)

    content_texts = [e.text for e in events if e.type == "Content"]
    assert "real reply" in content_texts
    assert "\n\n[Interrupted]" not in content_texts


@pytest.mark.asyncio
async def test_stream_voice_hint_passes_as_separate_field_on_first_turn():
    """First turn: voice_hint flows as a separate WS frame field (forwarded
    by Rust to chat_session_open as system_prompt). User message is NOT
    polluted with the hint — that was the old pageContext-prepend behavior
    we explicitly removed."""
    ws = AsyncMock()
    ws.send_text = AsyncMock()
    chat_rx: asyncio.Queue = asyncio.Queue()
    client = NeoMindCapabilityLLM(
        ws=ws, chat_rx=chat_rx,
        voice_hint="[MODE] be brief",
    )

    await chat_rx.put(_chunk("end"))

    async for _ in client.stream("hello", session_id="x"):
        pass

    sent = json.loads(ws.send_text.call_args.args[0])
    # User text is verbatim — no hint prepended.
    assert sent["message"] == "hello"
    # Hint flows in its own field for Rust to forward as system_prompt.
    assert sent["voice_hint"] == "[MODE] be brief"
    # No session_id yet on first turn (we haven't received the started frame).
    assert "session_id" not in sent


@pytest.mark.asyncio
async def test_stream_voice_hint_omitted_on_subsequent_turns():
    """Second+ turn: voice_hint must NOT be re-sent. The session already
    exists, so the hint lives in the LLM's system_prompt from initial
    creation. Re-sending would be a no-op (platform ignores it for existing
    sessions) but wastes bandwidth and clutters the wire trace."""
    ws = AsyncMock()
    ws.send_text = AsyncMock()
    chat_rx: asyncio.Queue = asyncio.Queue()
    client = NeoMindCapabilityLLM(
        ws=ws, chat_rx=chat_rx,
        voice_hint="[MODE] be brief",
    )
    # Simulate that we already captured a sessionId from a previous turn.
    client._neomind_session_id = "real-nm-sid"

    await chat_rx.put(_chunk("end"))

    async for _ in client.stream("hello again", session_id="x"):
        pass

    sent = json.loads(ws.send_text.call_args.args[0])
    assert sent["message"] == "hello again"
    assert sent["session_id"] == "real-nm-sid"
    assert "voice_hint" not in sent


@pytest.mark.asyncio
async def test_cancel_sends_chat_stream_cancel_with_captured_session_id():
    """cancel() sends chat_stream_cancel using the captured neomind session
    id (not the caller's voice pipeline id)."""
    ws = AsyncMock()
    ws.send_text = AsyncMock()
    chat_rx: asyncio.Queue = asyncio.Queue()
    client = NeoMindCapabilityLLM(ws=ws, chat_rx=chat_rx, voice_hint="")
    client._neomind_session_id = "real-nm-sid"

    await client.cancel("caller-ignored")

    sent = json.loads(ws.send_text.call_args.args[0])
    assert sent["type"] == "chat_stream_cancel"
    assert sent["session_id"] == "real-nm-sid"
    assert client._cancel_requested is True


@pytest.mark.asyncio
async def test_stream_terminates_on_cancel_flag():
    """If cancel() is called while stream() is awaiting a chunk, the next
    loop iteration yields 'end' immediately without consuming the queue."""
    ws = AsyncMock()
    ws.send_text = AsyncMock()
    chat_rx: asyncio.Queue = asyncio.Queue()
    client = NeoMindCapabilityLLM(ws=ws, chat_rx=chat_rx, voice_hint="")

    # No frames queued → stream() blocks on chat_rx.get().
    task = asyncio.create_task(_collect_events(client, "hi", "x"))
    await asyncio.sleep(0.01)  # let stream() enter the await
    await client.cancel("ignored")
    events = await task

    assert events and events[-1].type == "end"


async def _collect_events(client, user_text, session_id):
    out = []
    async for evt in client.stream(user_text, session_id=session_id):
        out.append(evt)
    return out


@pytest.mark.asyncio
async def test_make_llm_capability_requires_ws_and_queue():
    """Factory must refuse neomind_capability without ws+chat_rx — they're
    wired by ws_handler, and a missing arg indicates a misconfiguration."""
    from backends import make_llm
    from profile import Profile

    base = dict(
        name="test", vad_backend_type="silero",
        vad_config={"threshold": 0.5, "min_speech_ms": 250, "silence_ms": 500},
        asr_config={"type": "sensevoice_http", "url": "http://m"},
        llm_config={"type": "neomind_capability"},
        tts_config={"type": "zipvoice_http", "url": "http://m", "voice": "v"},
        aec_config=None, barge_in_mode="full",
        latency_target_ms=1000, cpu_threads=4,
        barge_in_ack=False, ack_words=["好的"],
        stage_filler_words={"thinking": ["让我想想"]},
        greeting_text="",
    )
    profile = Profile(**base)
    with pytest.raises(ValueError, match="ws.*chat_rx"):
        make_llm(profile)


@pytest.mark.asyncio
async def test_make_llm_capability_constructs_backend_with_ws_and_queue():
    """Happy-path: factory wires ws+chat_rx into NeoMindCapabilityLLM."""
    from backends import make_llm
    from profile import Profile

    base = dict(
        name="test", vad_backend_type="silero",
        vad_config={"threshold": 0.5, "min_speech_ms": 250, "silence_ms": 500},
        asr_config={"type": "sensevoice_http", "url": "http://m"},
        llm_config={"type": "neomind_capability", "voice_hint": ""},
        tts_config={"type": "zipvoice_http", "url": "http://m", "voice": "v"},
        aec_config=None, barge_in_mode="full",
        latency_target_ms=1000, cpu_threads=4,
        barge_in_ack=False, ack_words=["好的"],
        stage_filler_words={"thinking": ["让我想想"]},
        greeting_text="",
    )
    profile = Profile(**base)
    ws = AsyncMock()
    queue = asyncio.Queue()
    llm = make_llm(profile, ws=ws, chat_rx=queue)
    assert isinstance(llm, NeoMindCapabilityLLM)
    assert llm.ws is ws
    assert llm._chat_rx is queue
