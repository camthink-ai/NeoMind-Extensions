"""Profile YAML loading with env var override."""
from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path

import yaml


@dataclass
class Profile:
    name: str
    vad_backend_type: str
    vad_config: dict
    asr_config: dict
    llm_config: dict
    tts_config: dict
    aec_config: dict | None
    barge_in_mode: str
    latency_target_ms: int
    cpu_threads: int
    barge_in_ack: bool
    ack_words: list[str]
    stage_filler_words: dict[str, list[str]]

    @classmethod
    def from_dict(cls, d: dict) -> "Profile":
        acoustic = d.get("acoustic", {})
        backends = d.get("backends", {})
        interaction = d.get("interaction", {})
        hardware = d.get("hardware", {})
        aec = acoustic.get("aec", "none")
        # Stage fillers: short voice prompts played at different pipeline
        # stages so the user knows what's happening during slow LLM turns.
        # Supported stages: "thinking" (after ASR, before LLM), "tool_call"
        # (LLM emits ToolCallStart). Each stage has a word list; one is
        # picked at random when the stage fires.
        default_stage_fillers = {
            "thinking": ["让我想想", "嗯,让我想想"],
            "tool_call": ["我查一下", "搜索中"],
        }
        stage_filler_words = dict(interaction.get("stage_filler_words", default_stage_fillers))
        return cls(
            name=d.get("name", "unknown"),
            vad_backend_type=acoustic.get("vad_backend", "silero"),
            vad_config={
                "threshold": acoustic.get("vad_threshold", 0.5),
                "min_speech_ms": acoustic.get("vad_min_speech_ms", 250),
                "silence_ms": acoustic.get("vad_silence_ms", 500),
            },
            asr_config=dict(backends.get("asr", {})),
            llm_config=dict(backends.get("llm", {})),
            tts_config=dict(backends.get("tts", {})),
            aec_config=None if aec == "none" else {"type": aec},
            barge_in_mode=interaction.get("barge_in", "full"),
            latency_target_ms=interaction.get("latency_target_ms", 1200),
            cpu_threads=hardware.get("cpu_threads", 4),
            barge_in_ack=interaction.get("barge_in_ack", False),
            ack_words=list(interaction.get("ack_words", ["好的"])),
            stage_filler_words=stage_filler_words,
        )


def _parse_yaml(path: Path) -> dict:
    if not path.is_file():
        return {}
    return yaml.safe_load(path.read_text(encoding="utf-8")) or {}


def _deep_merge(base: dict, overlay: dict) -> dict:
    result = dict(base)
    for k, v in overlay.items():
        if k in result and isinstance(result[k], dict) and isinstance(v, dict):
            result[k] = _deep_merge(result[k], v)
        else:
            result[k] = v
    return result


def load_profile(name: str | None = None) -> Profile:
    """Load profile. Priority: env var > named profile > default."""
    profiles_dir = Path(__file__).parent / "profiles"
    default = _parse_yaml(profiles_dir / "default.yaml")
    if name:
        named = _parse_yaml(profiles_dir / f"{name}.yaml")
        merged = _deep_merge(default, named)
    else:
        merged = default

    acoustic = merged.setdefault("acoustic", {})
    backends = merged.setdefault("backends", {})
    tts = backends.setdefault("tts", {})

    # Env overrides
    if v := os.environ.get("VOICE_ASSISTANT_VAD_BACKEND"):
        acoustic["vad_backend"] = v.lower()
    if v := os.environ.get("VOICE_ASSISTANT_TTS_URL"):
        tts["url"] = v
    if v := os.environ.get("VOICE_ASSISTANT_VOICE"):
        tts["voice"] = v

    return Profile.from_dict(merged)
