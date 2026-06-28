"""Unit tests for ReferenceRingBuffer and AEC backends."""
import numpy as np
import pytest


# ---------------------------------------------------------------------------
# ReferenceRingBuffer
# ---------------------------------------------------------------------------

def test_ring_buffer_push_then_peek_full():
    """push(N bytes) then peek_window(0, full_duration_ms) returns those bytes."""
    from aec import ReferenceRingBuffer
    SAMPLE_RATE = 16000
    # 1 second capacity
    buf = ReferenceRingBuffer(capacity_bytes=SAMPLE_RATE * 2 * 1)
    # Push 100ms of distinctive samples
    samples = np.arange(1600, dtype="<i2")
    buf.push(samples.tobytes())
    out = buf.peek_window(delay_ms=0, length_ms=100, sample_rate=SAMPLE_RATE)
    out_samples = np.frombuffer(out, dtype="<i2")
    assert len(out_samples) == 1600
    assert (out_samples == samples).all()


def test_ring_buffer_delay_window():
    """peek_window(delay_ms=200, length_ms=100) returns the slice from 200ms ago."""
    from aec import ReferenceRingBuffer
    SAMPLE_RATE = 16000
    buf = ReferenceRingBuffer(capacity_bytes=SAMPLE_RATE * 2 * 2)  # 2s capacity
    # Push 300ms of zeros, then 100ms of ones (the "marker")
    buf.push(np.zeros(4800, dtype="<i2").tobytes())  # 300ms zeros
    marker = np.ones(1600, dtype="<i2")
    buf.push(marker.tobytes())  # 100ms ones at the end
    # Now: most-recent 100ms is the marker (delay=0); 200ms-100ms ago is also zeros
    # But we want 200ms ago for 100ms length — that's all zeros
    out = buf.peek_window(delay_ms=200, length_ms=100, sample_rate=SAMPLE_RATE)
    out_samples = np.frombuffer(out, dtype="<i2")
    assert len(out_samples) == 1600
    assert (out_samples == 0).all(), "200ms ago was still in the zeros region"


def test_ring_buffer_zero_pads_on_underflow():
    """When delay exceeds capacity, peek_window zero-pads missing prefix."""
    from aec import ReferenceRingBuffer
    SAMPLE_RATE = 16000
    # Tiny buffer: 100ms capacity
    buf = ReferenceRingBuffer(capacity_bytes=SAMPLE_RATE * 2 * 100 // 1000)
    buf.push(np.ones(1600, dtype="<i2").tobytes())  # 100ms of ones
    # Request 200ms window at delay=0; only 100ms is available, should zero-pad prefix
    out = buf.peek_window(delay_ms=0, length_ms=200, sample_rate=SAMPLE_RATE)
    out_samples = np.frombuffer(out, dtype="<i2")
    assert len(out_samples) == 3200
    # First 1600 should be zeros (padding), last 1600 should be ones
    assert (out_samples[:1600] == 0).all()
    assert (out_samples[1600:] == 1).all()


def test_ring_buffer_fifo_wrap():
    """Pushing beyond capacity drops oldest data (FIFO)."""
    from aec import ReferenceRingBuffer
    SAMPLE_RATE = 16000
    buf = ReferenceRingBuffer(capacity_bytes=SAMPLE_RATE * 2 * 200 // 1000)  # 200ms
    first = np.full(1600, 1, dtype="<i2")  # 100ms of 1s
    second = np.full(1600, 2, dtype="<i2")  # 100ms of 2s
    third = np.full(1600, 3, dtype="<i2")  # 100ms of 3s — should evict `first`
    buf.push(first.tobytes())
    buf.push(second.tobytes())
    buf.push(third.tobytes())
    out = buf.peek_window(delay_ms=0, length_ms=200, sample_rate=SAMPLE_RATE)
    out_samples = np.frombuffer(out, dtype="<i2")
    # `first` (1s) should be gone; we should see 2s then 3s
    assert (out_samples[:1600] == 2).all()
    assert (out_samples[1600:] == 3).all()


def test_ring_buffer_empty_peek_returns_zeros():
    """Empty buffer peek returns all zeros of the requested length."""
    from aec import ReferenceRingBuffer
    SAMPLE_RATE = 16000
    buf = ReferenceRingBuffer(capacity_bytes=SAMPLE_RATE * 2 * 1)
    out = buf.peek_window(delay_ms=0, length_ms=100, sample_rate=SAMPLE_RATE)
    out_samples = np.frombuffer(out, dtype="<i2")
    assert len(out_samples) == 1600
    assert (out_samples == 0).all()
