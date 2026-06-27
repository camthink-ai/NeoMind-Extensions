"""Tests for SentenceBuffer — sentence boundary splitting over LLM token stream."""
from __future__ import annotations

from sentence_buffer import SentenceBuffer


def test_chinese_period_splits():
    buf = SentenceBuffer()
    out = buf.feed("你好。")
    assert out == ["你好。"]


def test_english_period_splits():
    buf = SentenceBuffer()
    out = buf.feed("Hello world.")
    assert out == ["Hello world."]


def test_exclamation_and_question():
    buf = SentenceBuffer()
    assert buf.feed("太好了！") == ["太好了！"]
    # New buffer for the question (residual state would be empty anyway).
    out = buf.feed("怎么了?")
    assert out == ["怎么了?"]


def test_multiple_sentences_in_one_feed():
    buf = SentenceBuffer()
    out = buf.feed("你好。我是小明。今天天气不错！")
    assert out == ["你好。", "我是小明。", "今天天气不错！"]


def test_streaming_one_char_at_a_time():
    """Stream a sentence character by character — should emit only when complete."""
    buf = SentenceBuffer()
    emitted = []
    for ch in "你好世界。":
        emitted.extend(buf.feed(ch))
    # All emissions arrive together at the terminator (no partials before).
    assert emitted == ["你好世界。"]


def test_short_sentence_emitted_immediately():
    """Short sentences (e.g. '嗯。') emit immediately to keep bi-stream latency low."""
    buf = SentenceBuffer()
    out = buf.feed("嗯。")
    assert out == ["嗯。"]


def test_long_no_punctuation_hard_cut():
    buf = SentenceBuffer()
    long_text = "a" * (SentenceBuffer.MAX_CHARS + 20)
    out = buf.feed(long_text)
    # At least one soft-cut sentence emitted, none longer than MAX_CHARS.
    assert len(out) >= 1
    for s in out:
        assert len(s) <= SentenceBuffer.MAX_CHARS


def test_long_no_punct_soft_cut_at_space():
    buf = SentenceBuffer()
    # A long phrase with a space near the end of the limit window.
    text = "x" * 70 + " hello " + "y" * 30
    out = buf.feed(text)
    assert len(out) >= 1
    # First cut should land on the space (index >= 70).
    assert out[0].startswith("x" * 70)


def test_flush_returns_residue():
    buf = SentenceBuffer()
    buf.feed("你好。")
    # Remaining tail with no terminator.
    buf.feed("我是小明")
    tail = buf.flush()
    assert tail == "我是小明"
    # Subsequent flush returns None (buffer drained).
    assert buf.flush() is None


def test_flush_whitespace_only_returns_none():
    buf = SentenceBuffer()
    buf.feed("   \n\t  ")
    assert buf.flush() is None


def test_empty_and_partial_feeds():
    buf = SentenceBuffer()
    assert buf.feed("") == []
    assert buf.feed("部分") == []
    assert buf.flush() == "部分"


def test_newline_terminates():
    buf = SentenceBuffer()
    out = buf.feed("第一行\n第二行\n")
    assert out == ["第一行\n", "第二行\n"]
