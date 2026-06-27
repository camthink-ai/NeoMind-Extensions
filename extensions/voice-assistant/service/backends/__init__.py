"""Backend factory — Profile → concrete backend instances."""
from __future__ import annotations

from profile import Profile
from .vad import SileroVAD, EnergyVAD
from .asr import SenseVoiceHTTPASR
from .llm import OllamaHTTPClient, NeoMindWSClient, FakeLLMClient
from .tts import ZipVoiceHTTP


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
            return SenseVoiceHTTPASR(**cfg)
        case _:
            raise ValueError(f"unknown ASR backend: {t}")


def make_llm(profile: Profile):
    cfg = dict(profile.llm_config)
    t = cfg.pop("type")
    match t:
        case "neomind_ws":
            return NeoMindWSClient(**cfg)
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
            return ZipVoiceHTTP(**cfg)
        case _:
            raise ValueError(f"unknown TTS backend: {t}")
