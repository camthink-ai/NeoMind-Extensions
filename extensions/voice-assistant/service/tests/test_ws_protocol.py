"""WS protocol frame codec tests."""
from __future__ import annotations

import json

from ws_protocol import (
    encode_transcript, encode_phase, encode_stop, encode_error,
    encode_barge_in_ack, decode_start, decode_ping,
)


def test_encode_transcript():
    frame = encode_transcript("hello")
    obj = json.loads(frame)
    assert obj == {"type": "transcript", "text": "hello"}


def test_encode_phase():
    frame = encode_phase("asr_start", asr_ms=123.4)
    obj = json.loads(frame)
    assert obj["type"] == "asr_start"
    assert obj["asr_ms"] == 123.4


def test_encode_stop():
    assert json.loads(encode_stop()) == {"type": "stop"}


def test_encode_error():
    frame = encode_error("boom")
    assert json.loads(frame) == {"type": "error", "message": "boom"}


def test_decode_start():
    frame = json.dumps({
        "type": "start", "session_id": "s1",
        "sample_rate": 16000, "channels": 1, "format": "pcm_int16_le",
    })
    parsed = decode_start(frame)
    assert parsed["session_id"] == "s1"
    assert parsed["sample_rate"] == 16000


def test_decode_ping():
    assert decode_ping('{"type": "ping"}') is True
    assert decode_ping('{"type": "other"}') is False
