"""Backend factory — Profile → concrete backend instances."""
from __future__ import annotations

import logging

from profile import Profile
from .aec import NoopAECBackend, WebRtcAECBackend  # noqa: F401  (re-export)
from .vad import SileroVAD, EnergyVAD
from .asr import SenseVoiceHTTPASR, SenseVoiceInprocASR, Qwen3LlamaCppASR
from .llm import OllamaHTTPClient, NeoMindCapabilityLLM, NeoMindWSClient, FakeLLMClient
from .tts import ZipVoiceHTTP, ZipVoiceInprocTTS

logger = logging.getLogger(__name__)


def make_vad(profile: Profile):
    cfg = profile.vad_config
    match profile.vad_backend_type:
        case "silero":
            return SileroVAD(
                threshold=cfg["threshold"],
                min_speech_ms=cfg["min_speech_ms"],
                silence_ms=cfg["silence_ms"],
            )
        case "energy":
            return EnergyVAD(
                threshold=cfg["threshold"],
                min_speech_ms=cfg["min_speech_ms"],
                silence_ms=cfg["silence_ms"],
            )
        case _:
            raise ValueError(f"unknown VAD backend: {profile.vad_backend_type}")


def make_asr(profile: Profile):
    cfg = dict(profile.asr_config)
    t = cfg.pop("type")
    match t:
        # qwen3_asr_http shares the same /asr contract as sensevoice-asr;
        # the SenseVoiceHTTPASR class is a generic HTTP ASR client.
        case "sensevoice_http" | "qwen3_asr_http":
            # `num_threads` is a SenseVoiceInprocASR-only knob; it survives
            # profile deep_merge when default.yaml uses inproc but a named
            # profile switches to http. Pop so HTTP backend doesn't choke.
            cfg.pop("num_threads", None)
            # `streaming` is a Qwen3LlamaCppASR-only knob; the HTTP backend
            # is batched and ignores it. Pop to avoid TypeError.
            cfg.pop("streaming", None)
            return SenseVoiceHTTPASR(**cfg)
        case "sensevoice_inproc":
            return SenseVoiceInprocASR(**cfg)
        case "qwen3_llamacpp_asr":
            return Qwen3LlamaCppASR(**cfg)
        case _:
            raise ValueError(f"unknown ASR backend: {t}")


def make_llm(profile: Profile, ws=None, chat_rx=None):
    cfg = dict(profile.llm_config)
    t = cfg.pop("type")
    match t:
        case "neomind_ws":
            return NeoMindWSClient(**cfg)
        case "neomind_capability":
            # Capability path requires the live WS to the Rust extension and
            # the demultiplex queue fed by the main ws_handler receive loop.
            if ws is None or chat_rx is None:
                raise ValueError(
                    "neomind_capability LLM backend requires `ws` and `chat_rx` "
                    "(passed by server.ws_handler)"
                )
            # Pop neomind_ws-only fields that survive deep_merge from
            # default.yaml (url/token_env/auth_mode/voice_mode). Capability
            # path has no URL — host platform routes internally.
            for k in ("url", "token_env", "auth_mode", "voice_mode", "token"):
                cfg.pop(k, None)
            return NeoMindCapabilityLLM(ws=ws, chat_rx=chat_rx, **cfg)
        case "ollama_http":
            return OllamaHTTPClient(**cfg)
        case "fake":
            return FakeLLMClient(**cfg)
        case _:
            raise ValueError(f"unknown LLM backend: {t}")


def make_tts(profile: Profile):
    cfg = dict(profile.tts_config)
    t = cfg.pop("type")
    match t:
        # zipvoice_http / moss_tts_http / kokoro_http all share the same
        # NDJSON /tts/stream contract; the class name is historical.
        case "zipvoice_http" | "moss_tts_http" | "kokoro_http":
            # Pop inproc-only knobs that survive deep_merge from default.yaml.
            cfg.pop("num_threads", None)
            return ZipVoiceHTTP(**cfg)
        case "zipvoice_inproc":
            return ZipVoiceInprocTTS(**cfg)
        case _:
            raise ValueError(f"unknown TTS backend: {t}")


def make_aec(profile: Profile):
    """Construct an AECBackend from the profile. Always returns something
    usable; falls back to NoopAECBackend on import/init issues.

    type='none'/'echo_window' -> NoopAECBackend (echo_window is implemented
        in the VAD layer via _aec_active_now, not as a backend).
    type='webrtc' -> WebRtcAECBackend if library available, else Noop + warning.
    """
    cfg = profile.aec_config
    if cfg is None:
        return NoopAECBackend()
    t = cfg.get("type", "none")
    if t in ("none", "echo_window"):
        # echo_window is implemented as a VAD threshold boost in
        # server.py:VoiceSession._aec_active_now, not as an AEC backend.
        # Noop here; the VAD layer reads server.AEC_MODE to decide when
        # to apply the boost.
        return NoopAECBackend()
    if t == "webrtc":
        from backends import aec as aec_module
        if aec_module._resolve_webrtc_apm_class() is None:
            logger.warning(
                "AEC backend 'webrtc' unavailable (webrtc_audio_processing "
                "not installed); fallback to Noop"
            )
            return NoopAECBackend()
        return WebRtcAECBackend()
    raise ValueError(f"unknown AEC backend: {t}")
