"""Qwen3LlamaCppASR backend tests with mocked httpx.

Verifies:
  - transcribe() builds the right URL + payload and returns the message content
  - stream() parses SSE lines, accumulates token deltas, and emits a final
    PartialTranscript on [DONE]
"""
from __future__ import annotations

import json
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from backends.asr import Qwen3LlamaCppASR, PartialTranscript


def _sse_line(obj: dict) -> str:
    return f"data: {json.dumps(obj)}"


@pytest.mark.asyncio
async def test_transcribe_returns_message_content():
    backend = Qwen3LlamaCppASR(
        url="http://localhost:8080",
        model="qwen3-asr",
        language="auto",
        timeout=10.0,
    )

    fake_resp = MagicMock()
    fake_resp.raise_for_status = MagicMock()
    fake_resp.json = MagicMock(return_value={
        "choices": [{
            "message": {"content": "你好世界"},
        }],
    })

    fake_client = MagicMock()
    fake_client.__aenter__ = AsyncMock(return_value=fake_client)
    fake_client.__aexit__ = AsyncMock(return_value=None)
    fake_client.post = AsyncMock(return_value=fake_resp)

    with patch("backends.asr.httpx.AsyncClient", return_value=fake_client):
        result = await backend.transcribe([0.0] * 16000, 16000)

    assert result == "你好世界"
    # Confirm the request hit the OpenAI-compatible endpoint.
    fake_client.post.assert_called_once()
    url, = fake_client.post.call_args.args
    assert url == "http://localhost:8080/v1/chat/completions"
    payload = fake_client.post.call_args.kwargs["json"]
    assert payload["model"] == "qwen3-asr"
    assert payload["stream"] is False
    # Messages contain a data-URL image part.
    content = payload["messages"][0]["content"]
    assert any(
        p.get("type") == "image_url"
        and p["image_url"]["url"].startswith("data:audio/wav;base64,")
        for p in content
    )


@pytest.mark.asyncio
async def test_transcribe_handles_empty_choices():
    backend = Qwen3LlamaCppASR()
    fake_resp = MagicMock()
    fake_resp.raise_for_status = MagicMock()
    fake_resp.json = MagicMock(return_value={"choices": []})
    fake_client = MagicMock()
    fake_client.__aenter__ = AsyncMock(return_value=fake_client)
    fake_client.__aexit__ = AsyncMock(return_value=None)
    fake_client.post = AsyncMock(return_value=fake_resp)
    with patch("backends.asr.httpx.AsyncClient", return_value=fake_client):
        assert await backend.transcribe([0.0] * 100, 16000) == ""


@pytest.mark.asyncio
async def test_stream_accumulates_deltas_and_emits_final():
    """stream() must yield one PartialTranscript per delta with accumulated
    text, plus a final is_final=True marker after [DONE]."""
    backend = Qwen3LlamaCppASR(url="http://x", model="m", streaming=True)

    # SSE lines: three token deltas, then [DONE].
    sse_lines = [
        _sse_line({"choices": [{"delta": {"content": "你"}}]}),
        _sse_line({"choices": [{"delta": {"content": "好"}}]}),
        _sse_line({"choices": [{"delta": {"content": "世界"}}]}),
        "data: [DONE]",
    ]

    fake_stream_ctx = MagicMock()
    fake_stream_ctx.__aenter__ = AsyncMock(return_value=fake_stream_ctx)
    fake_stream_ctx.__aexit__ = AsyncMock(return_value=None)
    fake_stream_ctx.raise_for_status = MagicMock()
    fake_stream_ctx.aiter_lines = MagicMock(
        return_value=_async_iter(sse_lines)
    )

    fake_client = MagicMock()
    fake_client.__aenter__ = AsyncMock(return_value=fake_client)
    fake_client.__aexit__ = AsyncMock(return_value=None)
    fake_client.stream = MagicMock(return_value=fake_stream_ctx)

    with patch("backends.asr.httpx.AsyncClient", return_value=fake_client):
        parts = []
        async for pt in backend.stream([0.0] * 100, 16000):
            parts.append(pt)

    # Three partials + one final.
    assert len(parts) == 4
    assert [p.text for p in parts[:3]] == ["你", "你好", "你好世界"]
    assert all(not p.is_final for p in parts[:3])
    assert parts[-1].is_final is True
    assert parts[-1].text == "你好世界"
    assert parts[-1].confidence == 1.0

    # Confirm stream=True was passed.
    fake_client.stream.assert_called_once()
    method, url = fake_client.stream.call_args.args
    assert method == "POST"
    assert url == "http://x/v1/chat/completions"
    payload = fake_client.stream.call_args.kwargs["json"]
    assert payload["stream"] is True


@pytest.mark.asyncio
async def test_stream_skips_lines_without_content():
    """Comments (``: ping``), empty lines, and choices without delta.content
    must be ignored, not yielded as empty partials."""
    backend = Qwen3LlamaCppASR(streaming=True)
    sse_lines = [
        "",
        ": keep-alive",
        _sse_line({"choices": [{"delta": {}}]}),  # no content
        _sse_line({"choices": [{"delta": {"content": "ok"}}]}),
        "data: [DONE]",
    ]

    fake_stream_ctx = MagicMock()
    fake_stream_ctx.__aenter__ = AsyncMock(return_value=fake_stream_ctx)
    fake_stream_ctx.__aexit__ = AsyncMock(return_value=None)
    fake_stream_ctx.raise_for_status = MagicMock()
    fake_stream_ctx.aiter_lines = MagicMock(return_value=_async_iter(sse_lines))

    fake_client = MagicMock()
    fake_client.__aenter__ = AsyncMock(return_value=fake_client)
    fake_client.__aexit__ = AsyncMock(return_value=None)
    fake_client.stream = MagicMock(return_value=fake_stream_ctx)

    with patch("backends.asr.httpx.AsyncClient", return_value=fake_client):
        parts = [pt async for pt in backend.stream([0.0] * 10, 16000)]

    # Only "ok" delta + final.
    assert [p.text for p in parts] == ["ok", "ok"]
    assert parts[0].is_final is False
    assert parts[1].is_final is True


async def _async_iter(lines):
    """Build an async iterator over a static list of strings."""
    for line in lines:
        yield line
