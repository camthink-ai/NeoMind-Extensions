"""Unit tests for the greeting (say-first) feature."""
from __future__ import annotations

from profile import Profile


def test_greeting_text_defaults_to_empty():
    """Profile without greeting_text in YAML defaults to empty string."""
    p = Profile.from_dict({})
    assert p.greeting_text == ""


def test_greeting_text_loaded_from_interaction_dict():
    """Profile reads greeting_text from interaction.* block."""
    p = Profile.from_dict({"interaction": {"greeting_text": "你好"}})
    assert p.greeting_text == "你好"


def test_greeting_text_whitespace_preserved():
    """Whitespace-only greeting_text is preserved as-is (empty check happens
    in _warm_greeting via .strip(), not in Profile.from_dict)."""
    p = Profile.from_dict({"interaction": {"greeting_text": "  hi  "}})
    assert p.greeting_text == "  hi  "
