"""SenseVoiceHTTPASR backend test with mocked HTTP."""
from __future__ import annotations

import json
from unittest.mock import AsyncMock, patch

import pytest

from backends.asr import SenseVoiceHTTPASR


@pytest.mark.asyncio
async def test_transcribe_returns_text():
    asr = SenseVoiceHTTPASR(url="http://mock:9383", language="auto")
    mock_response = AsyncMock()
    mock_response.status_code = 200
    mock_response.json = lambda: {"text": "你好世界"}
    mock_response.raise_for_status = lambda: None

    with patch("httpx.AsyncClient.post", new=AsyncMock(return_value=mock_response)):
        result = await asr.transcribe([0.0, 0.1, 0.2], 16000)
    assert result == "你好世界"


@pytest.mark.asyncio
async def test_transcribe_raises_on_http_error():
    import httpx
    asr = SenseVoiceHTTPASR(url="http://mock:9383")
    mock_response = AsyncMock()
    mock_response.status_code = 500
    mock_response.raise_for_status = lambda: (_ for _ in ()).throw(
        httpx.HTTPStatusError("err", request=None, response=mock_response)
    )
    with patch("httpx.AsyncClient.post", new=AsyncMock(return_value=mock_response)):
        with pytest.raises(Exception):
            await asr.transcribe([0.0], 16000)
