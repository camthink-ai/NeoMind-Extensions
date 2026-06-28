"""Server-side AEC reference ring buffer.

Stores the last N seconds of server-pushed PCM (TTS + greeting + ack +
stage-filler) so the AEC backend can subtract the speaker echo from
upstream mic PCM.
"""
from __future__ import annotations

import numpy as np


class ReferenceRingBuffer:
    """Fixed-capacity int16 LE mono PCM ring buffer."""

    def __init__(self, capacity_bytes: int) -> None:
        # Round to even (int16 sample boundary)
        capacity_bytes = int(capacity_bytes)
        self._capacity = capacity_bytes - (capacity_bytes % 2)
        self._buf = bytearray(self._capacity)
        self._write_pos = 0  # next byte index to write
        self._filled = 0     # bytes ever written, capped at capacity

    def push(self, pcm_int16_bytes: bytes) -> None:
        """Append PCM to the ring. Wraps FIFO; overwrites oldest data."""
        if not pcm_int16_bytes:
            return
        # Drop trailing byte if odd-length — int16 samples require even byte count.
        # This is defensive; well-formed callers always send whole-sample PCM.
        if len(pcm_int16_bytes) % 2 != 0:
            pcm_int16_bytes = pcm_int16_bytes[:-1]
        data = pcm_int16_bytes
        n = len(data)
        if n >= self._capacity:
            # Only the most recent capacity-bytes matter
            data = data[-self._capacity:]
            n = self._capacity
        # Two-step copy for the wrap
        end = self._write_pos + n
        if end <= self._capacity:
            self._buf[self._write_pos:end] = data
        else:
            first_chunk = self._capacity - self._write_pos
            self._buf[self._write_pos:] = data[:first_chunk]
            self._buf[:end - self._capacity] = data[first_chunk:]
        self._write_pos = (self._write_pos + n) % self._capacity
        self._filled = min(self._capacity, self._filled + n)

    def peek_window(self, delay_ms: float, length_ms: float,
                    sample_rate: int = 16000) -> bytes:
        """Return length_ms of PCM from delay_ms ago, as int16 LE bytes.

        Zero-pads the prefix if the requested window underflows the
        available history (delay exceeds what's buffered).
        """
        length_samples = int(sample_rate * length_ms / 1000)
        delay_samples = int(sample_rate * delay_ms / 1000)
        out = np.zeros(length_samples, dtype="<i2")
        if self._filled == 0 or delay_samples >= length_samples + self._filled // 2:
            return out.tobytes()
        # How many actual samples can we read?
        available_after_delay = max(0, self._filled // 2 - delay_samples)
        readable = min(length_samples, available_after_delay)
        if readable <= 0:
            return out.tobytes()
        # Compute read start position (in samples)
        # Most-recent sample is at write_pos - 1 (going backwards).
        # Sample at delay_samples ago is at write_pos - 1 - delay_samples.
        # We want a contiguous window [delay_samples+readable-1, delay_samples] ago,
        # read forward in time.
        read_end_sample = (self._write_pos // 2) - delay_samples  # exclusive
        read_start_sample = read_end_sample - readable
        # Out samples layout: zeros for the unreadable prefix, then real data.
        zero_prefix_samples = length_samples - readable
        for i in range(readable):
            src = (read_start_sample + i) % (self._capacity // 2)
            out[zero_prefix_samples + i] = int.from_bytes(
                self._buf[src * 2: src * 2 + 2], byteorder="little", signed=True
            )
        return out.tobytes()
