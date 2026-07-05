"""HTTP /config endpoint tests.

Validates GET /config + POST /config (instant overrides + reload trigger).
Uses FastAPI TestClient + mocked ASR/TTS backends to avoid loading real
sherpa_onnx models.
"""
from __future__ import annotations

import os
from unittest.mock import MagicMock

import pytest
from fastapi.testclient import TestClient


@pytest.fixture
def client(monkeypatch):
    """TestClient with mocked ASR/TTS so we don't load sherpa_onnx."""
    import server

    mock_asr = MagicMock()
    mock_asr.language = "auto"
    mock_asr.transcribe = MagicMock()
    monkeypatch.setattr(server, "_asr_backend", mock_asr)

    mock_tts = MagicMock()
    mock_tts.voice = "中文女"
    monkeypatch.setattr(server, "_tts_backend", mock_tts)

    # Clean NEOMIND_TOKEN env for deterministic token tests.
    monkeypatch.delenv("NEOMIND_TOKEN", raising=False)

    return TestClient(server.app), server


# ---------------------------------------------------------------------------
# GET /config
# ---------------------------------------------------------------------------

def test_get_config_returns_current_state_and_options(client):
    c, _ = client
    resp = c.get("/config")
    assert resp.status_code == 200
    body = resp.json()
    assert "current" in body
    assert "available_profiles" in body
    assert "available_languages" in body
    assert "reloading" in body
    # Profile list is non-empty (at least 'default')
    assert "default" in body["available_profiles"]
    # Languages include the documented set
    for lang in ("auto", "zh", "en"):
        assert lang in body["available_languages"]
    # Current has the expected keys
    cur = body["current"]
    for k in ("profile", "language", "voice", "neoMindTokenMasked",
              "neoMindTokenSet", "asrType", "ttsType", "llmType", "numThreads"):
        assert k in cur


def test_get_config_masks_token(client, monkeypatch):
    c, server = client
    monkeypatch.setenv("NEOMIND_TOKEN", "nmk_abcdef12345")
    resp = c.get("/config")
    cur = resp.json()["current"]
    assert cur["neoMindTokenSet"] is True
    # Masked form should NOT contain the full token
    assert "abcdef12345" not in cur["neoMindTokenMasked"]
    assert cur["neoMindTokenMasked"].endswith("***")


# ---------------------------------------------------------------------------
# POST /config — instant overrides
# ---------------------------------------------------------------------------

def test_post_language_applies_instantly(client):
    c, server = client
    resp = c.post("/config", json={"language": "en"})
    assert resp.status_code == 200
    body = resp.json()
    assert "language" in body["applied"]
    assert body["reloaded"] is False
    # _profile + backend instance attr both updated
    assert server._profile.asr_config["language"] == "en"
    assert server._asr_backend.language == "en"


def test_post_voice_applies_instantly(client):
    c, server = client
    resp = c.post("/config", json={"voice": "中文男"})
    assert resp.status_code == 200
    assert body_ok(resp.json(), "voice")
    assert server._profile.tts_config["voice"] == "中文男"
    assert server._tts_backend.voice == "中文男"
    # TTS_VOICE backward-compat global updates too
    assert server.TTS_VOICE == "中文男"


def test_post_neomind_token_updates_env(client):
    c, server = client
    resp = c.post("/config", json={"neoMindToken": "nmk_secret_xyz"})
    assert resp.status_code == 200
    assert body_ok(resp.json(), "neoMindToken")
    # Token persisted to env var named by profile's token_env field
    token_env = server._profile.llm_config.get("token_env", "NEOMIND_TOKEN")
    assert os.environ.get(token_env) == "nmk_secret_xyz"


def test_post_empty_payload_is_noop(client):
    c, server = client
    before = server._profile.name
    resp = c.post("/config", json={})
    assert resp.status_code == 200
    body = resp.json()
    assert body["applied"] == []
    assert body["reloaded"] is False
    assert server._profile.name == before


def test_post_empty_token_is_ignored(client):
    """An empty token must NOT wipe the current env value (user likely meant
    'no change' rather than 'clear')."""
    c, server = client
    os.environ["NEOMIND_TOKEN"] = "nmk_keep_me"
    resp = c.post("/config", json={"neoMindToken": ""})
    assert resp.status_code == 200
    assert "neoMindToken" not in resp.json()["applied"]
    assert os.environ.get("NEOMIND_TOKEN") == "nmk_keep_me"


# ---------------------------------------------------------------------------
# POST /config — reload trigger (profile / numThreads)
# ---------------------------------------------------------------------------

def test_post_profile_switch_triggers_reload(client, monkeypatch):
    c, server = client
    # Snapshot module globals so we can restore them in teardown — reload
    # mutates server._profile / _vad_backend / etc. directly (not via
    # monkeypatch), and leftover mock backends would break later tests
    # (e.g. ws_integration) that reuse the module.
    snapshot = {
        "_profile": server._profile,
        "_vad_backend": server._vad_backend,
        "_asr_backend": server._asr_backend,
        "_tts_backend": server._tts_backend,
        "ASR_URL": server.ASR_URL,
        "TTS_URL": server.TTS_URL,
        "TTS_VOICE": server.TTS_VOICE,
        "VAD_BACKEND": server.VAD_BACKEND,
        "_ACK_PCM_BANK": list(server._ACK_PCM_BANK),
        "_STAGE_FILLER_BANK": dict(server._STAGE_FILLER_BANK),
        "_ACK_BANK_WARMED": server._ACK_BANK_WARMED,
        "_STAGE_BANK_WARMED": server._STAGE_BANK_WARMED,
        "_GREETING_PCM": server._GREETING_PCM,
    }

    try:
        # Stub make_vad/asr/tts so reload doesn't try to load real models.
        vad2 = MagicMock()
        asr2 = MagicMock()
        tts2 = MagicMock()
        monkeypatch.setattr(server, "make_vad", lambda p: vad2)
        monkeypatch.setattr(server, "make_asr", lambda p: asr2)
        monkeypatch.setattr(server, "make_tts", lambda p: tts2)
        # Stub _warm_banks_async so reload doesn't spawn a background task
        # that would outlive the test.
        async def _noop_warm():
            return None
        monkeypatch.setattr(server, "_warm_banks_async", _noop_warm)

        resp = c.post("/config", json={"profile": "noisy-env"})
        assert resp.status_code == 200
        body = resp.json()
        assert body["reloaded"] is True
        assert "profile" in body["applied"]
        assert body["reload_seconds"] is not None
        assert server._profile.name == "noisy-env"
        assert server._vad_backend is vad2
        assert server._asr_backend is asr2
        assert server._tts_backend is tts2
    finally:
        # Restore module globals mutated by reload.
        server._profile = snapshot["_profile"]
        server._vad_backend = snapshot["_vad_backend"]
        server._asr_backend = snapshot["_asr_backend"]
        server._tts_backend = snapshot["_tts_backend"]
        server.ASR_URL = snapshot["ASR_URL"]
        server.TTS_URL = snapshot["TTS_URL"]
        server.TTS_VOICE = snapshot["TTS_VOICE"]
        server.VAD_BACKEND = snapshot["VAD_BACKEND"]
        server._ACK_PCM_BANK.clear()
        server._ACK_PCM_BANK.extend(snapshot["_ACK_PCM_BANK"])
        server._STAGE_FILLER_BANK.clear()
        server._STAGE_FILLER_BANK.update(snapshot["_STAGE_FILLER_BANK"])
        server._ACK_BANK_WARMED = snapshot["_ACK_BANK_WARMED"]
        server._STAGE_BANK_WARMED = snapshot["_STAGE_BANK_WARMED"]
        server._GREETING_PCM = snapshot["_GREETING_PCM"]


def test_post_same_profile_is_noop(client, monkeypatch):
    """Reposting the current profile name must NOT trigger a reload."""
    c, server = client
    monkeypatch.setattr(server, "make_vad", lambda *a, **kw: pytest.fail("reload fired"))
    resp = c.post("/config", json={"profile": server._profile.name})
    body = resp.json()
    assert body["reloaded"] is False


def test_post_returns_503_while_reloading(client, monkeypatch):
    """If _reloading is True, POST /config returns 503."""
    c, server = client
    monkeypatch.setattr(server, "_reloading", True)
    resp = c.post("/config", json={"language": "en"})
    assert resp.status_code == 503
    assert resp.json()["error"] == "reload_in_progress"


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------

def body_ok(body: dict, field: str) -> bool:
    return field in body["applied"] and body["reloaded"] is False
