"""ZipVoiceHTTP backend test with mocked HTTP."""
from __future__ import annotations

import base64
import json
from unittest.mock import AsyncMock, patch

import pytest

from backends.tts import ZipVoiceHTTP


@pytest.mark.asyncio
async def test_synthesize_returns_pcm():
    tts = ZipVoiceHTTP(url="http://mock:9386", voice="中文女")
    fake_pcm = b"\x00\x00" * 100
    fake_ndjson = (
        json.dumps({
            "seq": 0,
            "data": base64.b64encode(fake_pcm).decode(),
            "sample_rate": 24000,
            "channels": 1,
        }).encode() + b"\n"
    )

    mock_resp = AsyncMock()
    mock_resp.status_code = 200
    mock_resp.raise_for_status = lambda: None
    mock_resp.aiter_lines = lambda: _aiter_lines([fake_ndjson.decode()])
    mock_resp.__aenter__ = AsyncMock(return_value=mock_resp)
    mock_resp.__aexit__ = AsyncMock(return_value=None)

    with patch("httpx.AsyncClient.stream", return_value=mock_resp):
        pcm = await tts.synthesize("你好", "中文女")
    assert pcm == fake_pcm


async def _aiter_lines(lines):
    """Helper for async iteration over lines."""
    for line in lines:
        yield line
