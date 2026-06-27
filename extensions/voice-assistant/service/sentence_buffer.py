"""Sentence boundary buffer for streaming LLM → TTS pipelining.

Accumulates token chunks from an LLM stream and emits complete sentences
as soon as a terminator (。!?!.?…\\n) is seen. Falls back to a soft cut
at the last space/commas when ``MAX_CHARS`` is exceeded without any
terminator, so a runaway LLM without punctuation can't stall TTS.

The buffer is sync and stateless across turns — ``flush()`` resets it.
"""
from __future__ import annotations


class SentenceBuffer:
    END_PUNCT = set("。!?!.?…\n！？")  # ASCII + CJK fullwidth variants
    MAX_CHARS = 80
    # Hard-cut fallback index when no space/comma is found in an over-long
    # unpunctuated run — keeps cuts deterministic rather than at position 0.
    SOFT_CUT_MIN = 40

    def __init__(self) -> None:
        self._buf = ""

    def feed(self, token: str) -> list[str]:
        """Accept one token; return 0..N complete sentences emitted this call.

        Emits on the earliest terminator (。!?!.?…\\n). When the accumulated
        buffer exceeds ``MAX_CHARS`` without any terminator, performs a soft
        cut at the nearest space / fullwidth comma — falling back to a hard
        cut at ``SOFT_CUT_MIN`` — so a runaway LLM without punctuation can't
        stall TTS playback indefinitely.
        """
        if not token:
            return []
        self._buf += token
        out: list[str] = []
        while True:
            # 1) Earliest terminator wins.
            cut = -1
            for i, ch in enumerate(self._buf):
                if ch in self.END_PUNCT:
                    cut = i
                    break
            if cut >= 0:
                out.append(self._buf[: cut + 1])
                self._buf = self._buf[cut + 1 :]
                continue
            # 2) No punctuation but over the safety limit → soft cut at the
            # nearest space / fullwidth comma, falling back to a hard cut.
            if len(self._buf) >= self.MAX_CHARS:
                soft = max(
                    self._buf.rfind(" "),
                    self._buf.rfind("，"),
                    self._buf.rfind(","),
                    self.SOFT_CUT_MIN,
                )
                out.append(self._buf[:soft])
                self._buf = self._buf[soft:]
                continue
            break
        return out

    def flush(self) -> str | None:
        """Return any trailing residual text after the LLM stream ends.

        Returns ``None`` for empty/whitespace-only residue so callers can
        treat the return value uniformly as "optional last sentence".
        """
        s, self._buf = self._buf, ""
        return s.strip() or None
