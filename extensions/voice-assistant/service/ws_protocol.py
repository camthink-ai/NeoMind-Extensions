"""Browser WS frame encoder/decoder. Single source of truth for protocol."""
from __future__ import annotations

import json


def encode_transcript(text: str) -> str:
    return json.dumps({"type": "transcript", "text": text}, ensure_ascii=False)


def encode_phase(phase: str, **metrics) -> str:
    """phase: 'asr_start' | 'asr_end' | 'tts_start' | 'tts_end'."""
    obj = {"type": phase}
    obj.update(metrics)
    return json.dumps(obj, ensure_ascii=False)


def encode_stop() -> str:
    return json.dumps({"type": "stop"})


def encode_error(message: str) -> str:
    return json.dumps({"type": "error", "message": message}, ensure_ascii=False)


def encode_barge_in_ack() -> str:
    return json.dumps({"type": "control", "action": "stop_playback",
                       "reason": "barge_in"})


def decode_start(frame_text: str) -> dict:
    """Parse a 'start' text frame from browser."""
    obj = json.loads(frame_text)
    return obj  # caller checks type == "start"


def decode_ping(frame_text: str) -> bool:
    try:
        return json.loads(frame_text).get("type") == "ping"
    except (json.JSONDecodeError, AttributeError):
        return False
