"""Unit tests for the NDJSON /tts/stream contract — run WITHOUT sherpa-onnx
installed by monkeypatching `tts` global. This validates response shape only;
end-to-end quality is covered by the manual test in Task A5 Step 5.
"""
import base64
import json
from unittest.mock import MagicMock, patch

import numpy as np
import pytest
from fastapi.testclient import TestClient


@pytest.fixture
def client(monkeypatch):
    import server as srv

    # Fake a "loaded" tts that emits 0.5s of silence at 24kHz.
    fake_audio = MagicMock()
    fake_audio.samples = np.zeros(12000, dtype=np.float32)  # 0.5s @ 24kHz
    fake_audio.sample_rate = 24000
    fake_tts = MagicMock()
    fake_tts.generate.return_value = fake_audio

    monkeypatch.setattr(srv, "tts", fake_tts)
    monkeypatch.setattr(srv, "available_voices", ["中文女"])
    monkeypatch.setattr(srv, "_default_prompt_wav", "/dev/null")
    monkeypatch.setattr(srv, "_default_prompt_text", "test")
    return TestClient(srv.app)


def test_health(client):
    r = client.get("/health")
    assert r.status_code == 200
    body = r.json()
    assert body["status"] == "ok"
    assert body["sample_rate"] == 24000
    assert "中文女" in body["voices"]


def test_voices(client):
    r = client.get("/voices")
    assert r.status_code == 200
    assert "中文女" in r.json()["voices"]


def test_stream_ndjson_shape(client):
    """The critical contract test: NDJSON line must have exactly the 5 keys
    voice-assistant's tts_stream() parser expects."""
    with patch("server._load_prompt") as fake_load:
        fake_load.return_value = ([0.0] * 16000, 16000)
        r = client.post("/tts/stream", json={"text": "你好", "voice": "中文女"})
    assert r.status_code == 200
    lines = [json.loads(line) for line in r.text.strip().split("\n") if line]
    assert len(lines) >= 1, "expected at least one NDJSON line"
    chunk = lines[0]
    expected_keys = {"seq", "data", "sample_rate", "channels", "is_pause"}
    assert set(chunk.keys()) == expected_keys, (
        f"NDJSON keys mismatch. got {set(chunk.keys())}, expected {expected_keys}"
    )
    assert chunk["seq"] == 0
    assert chunk["sample_rate"] == 24000
    assert chunk["channels"] == 1
    assert chunk["is_pause"] is False
    # base64 round-trip → PCM bytes of even length (int16 samples)
    pcm = base64.b64decode(chunk["data"])
    assert len(pcm) > 0
    assert len(pcm) % 2 == 0, "PCM byte length must be even (int16 LE)"


def test_stream_empty_text_still_produces_chunk(client):
    """Even with empty text, the service should emit one (possibly silent) chunk
    rather than hanging — voice-assistant's consumer expects at least one line
    or it will block indefinitely."""
    with patch("server._load_prompt") as fake_load:
        fake_load.return_value = ([0.0] * 16000, 16000)
        r = client.post("/tts/stream", json={"text": "", "voice": "中文女"})
    assert r.status_code == 200
    lines = [json.loads(line) for line in r.text.strip().split("\n") if line]
    assert len(lines) >= 1


def test_tts_full_wav_has_headers(client):
    """/tts (non-stream) must return WAV bytes with X-* timing headers."""
    with patch("server._load_prompt") as fake_load:
        fake_load.return_value = ([0.0] * 16000, 16000)
        r = client.post("/tts", json={"text": "测试", "voice": "中文女"})
    assert r.status_code == 200
    assert r.headers["X-Sample-Rate"] == "24000"
    assert r.headers["X-Channels"] == "1"
    assert "X-Elapsed-Seconds" in r.headers
    assert "X-Duration-Seconds" in r.headers
    # WAV magic bytes
    assert r.content[:4] == b"RIFF"
    assert r.content[8:12] == b"WAVE"
