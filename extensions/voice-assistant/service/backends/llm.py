"""LLM backend implementations."""
from __future__ import annotations

import asyncio
import json
import logging
import os
from typing import AsyncIterator

import httpx

from contracts import LlmEvent

logger = logging.getLogger("voice-assistant.llm")


class FakeLLMClient:
    """Echo LLM for testing. Implements LLMClient Protocol.

    Yields LlmEvent(type="Content") chunks then a final LlmEvent(type="end").
    Note: this yields raw token chunks, NOT sentence-split fragments.
    The orchestrator (Task 14) is responsible for sentence splitting.
    """

    def __init__(self, reply_template: str = "你刚才说: {text}"):
        self.reply_template = reply_template
        self._cancelled_sessions: set[str] = set()

    async def stream(self, user_text: str, session_id: str) -> AsyncIterator[LlmEvent]:
        if session_id in self._cancelled_sessions:
            yield LlmEvent(type="end")
            return
        reply = self.reply_template.format(text=user_text)
        # Emit Content in ~10-char chunks to simulate streaming tokens
        for i in range(0, len(reply), 10):
            if session_id in self._cancelled_sessions:
                yield LlmEvent(type="end")
                return
            chunk = reply[i:i + 10]
            yield LlmEvent(type="Content", text=chunk)
            await asyncio.sleep(0.01)
        yield LlmEvent(type="end")

    async def cancel(self, session_id: str) -> None:
        self._cancelled_sessions.add(session_id)


class OllamaHTTPClient:
    """Ollama local LLM via HTTP streaming (port 11434).

    Streams Content events (one per token) from Ollama's /api/chat NDJSON,
    then a final 'end' event. Implements the LLMClient Protocol.

    The orchestrator (Task 14) is responsible for sentence-splitting the
    Content events before forwarding to TTS.
    """

    def __init__(
        self,
        url: str = "http://127.0.0.1:11434",
        model: str = "qwen3:1.7b",
        system_prompt: str | None = None,
        timeout: float = 60.0,
        **_extra,
    ):
        self.url = url
        self.model = model
        self.system_prompt = system_prompt or (
            "你是简洁的中文语音助手。用口语化短句回答，每句不超过 20 字。"
            "不要使用 Markdown 格式。"
        )
        self.timeout = timeout
        self._cancelled: set[str] = set()

    async def stream(self, user_text: str, session_id: str) -> AsyncIterator[LlmEvent]:
        """Stream Content events from Ollama, then a final 'end' event.

        Checks self._cancelled between tokens; if cancelled, emits 'end' and returns.
        """
        try:
            async with httpx.AsyncClient(timeout=self.timeout) as client:
                async with client.stream(
                    "POST",
                    f"{self.url}/api/chat",
                    json={
                        "model": self.model,
                        "messages": [
                            {"role": "system", "content": self.system_prompt},
                            {"role": "user", "content": user_text},
                        ],
                        "stream": True,
                        "think": False,
                    },
                ) as resp:
                    resp.raise_for_status()
                    async for line in resp.aiter_lines():
                        if session_id in self._cancelled:
                            yield LlmEvent(type="end")
                            return
                        if not line.strip():
                            continue
                        try:
                            obj = json.loads(line)
                        except Exception:
                            continue
                        if obj.get("done"):
                            yield LlmEvent(type="end")
                            return
                        chunk = obj.get("message", {}).get("content", "")
                        if chunk:
                            yield LlmEvent(type="Content", text=chunk)
        except httpx.HTTPError as e:
            yield LlmEvent(type="Content", text=f"(LLM 错误: {e})")
            yield LlmEvent(type="end")

    async def cancel(self, session_id: str) -> None:
        self._cancelled.add(session_id)


class NeoMindWSClient:
    """NeoMind chat via WebSocket. Implements LLMClient Protocol.

    Protocol (verified from NeoMind source `neomind-api/src/models/mod.rs`):
    - Connect to ws://host:port/api/chat?api_key=<nmk_...>
    - Send {"message": user_text, "sessionId": ...}  (ChatRequest struct)
    - Receive events: system / ping / session_created / Intent / Plan /
      Content / Thinking / ToolCallStart / ToolCallEnd / Progress / end / Error
    - To cancel: send {"message": "__CANCEL__", "sessionId": ...}
    """

    INTERRUPTED_MARKER = "\n\n[Interrupted]"

    # Voice hint used as the LLM's `system_prompt` for new sessions.
    # Injected ONCE at session creation via ChatRequest.sessionConfig
    # (replaces the older pageContext prepend that polluted every user
    # message). Override per-profile via `voice_hint:` under llm config,
    # or set to empty string to disable.
    DEFAULT_VOICE_HINT = (
        "[语音助手模式] 请用1-3个短句口语化回答,不要 markdown,不要分点列清单,"
        "不要重复用户问题,总字数控制在80字以内。如果用户问题简单,回答也要简短。"
    )

    def __init__(
        self,
        url: str,
        token: str | None = None,
        token_env: str = "NEOMIND_TOKEN",
        auth_mode: str = "api_key",
        voice_mode: bool = True,
        timeout: float = 60.0,
        voice_hint: str | None = None,
    ):
        """auth_mode: 'api_key' uses ?api_key=xxx (NeoMind API key, nmk_...).
        'token' uses ?token=xxx (NeoMind JWT). Default 'api_key' since the
        CLI-issued keys (neomind api-key create) are API keys.

        voice_hint: applied as system_prompt when this client creates a new
        NeoMind session (first turn only). None → DEFAULT_VOICE_HINT.
        Empty string → disable injection.
        """
        self.url = url
        self.token = token or os.environ.get(token_env, "")
        self.auth_mode = auth_mode
        self.voice_mode = voice_mode
        self.timeout = timeout
        self.voice_hint = self.DEFAULT_VOICE_HINT if voice_hint is None else voice_hint
        self._active_ws = None
        self._llm_completed = False
        # NeoMind assigns a real sessionId on first message (`session_created`
        # event). Capture and reuse so multi-turn voice conversations map to
        # ONE NeoMind session (preserves LLM history, shows up as one thread
        # in the Tauri UI). Cleared on close().
        self._neomind_session_id: str | None = None

    async def stream(self, user_text: str, session_id: str) -> AsyncIterator[LlmEvent]:
        import websockets

        self._llm_completed = False
        if self.token:
            param = "api_key" if self.auth_mode == "api_key" else "token"
            url = f"{self.url}?{param}={self.token}"
        else:
            url = self.url
        async with websockets.connect(url, max_size=2**24) as ws:
            self._active_ws = ws
            # NeoMind chat WS protocol (per ChatRequest struct in
            # neomind-api/src/models/mod.rs): field is `message`, NOT `content`.
            # `type` is not in the schema and would be silently ignored.
            # sessionConfig (when present) is honored ONLY when this frame
            # triggers session creation — server-side plumbing in
            # handlers/sessions.rs auto-create branch translates
            # SessionConfigPatch → CreateSessionOptions → per-session
            # AgentConfig.system_prompt.
            #
            # sessionId: prefer the server-assigned id captured from a prior
            # `session_created` event. Ignoring the caller's `session_id` arg
            # (which today is `str(id(pipeline))` — a Python object id, not a
            # real NeoMind session uuid). Passing None on first turn lets the
            # server mint a fresh uuid; we capture it below.
            effective_sid = self._neomind_session_id or None
            payload = {
                "message": user_text,
                "sessionId": effective_sid,
            }
            # Voice hint → system_prompt. Only sent on the FIRST turn of a
            # conversation (when we have no captured sessionId yet). Once
            # the session exists, the platform ignores sessionConfig
            # anyway (safety property from PR1), so sending it would just
            # waste bandwidth and clutter the wire trace.
            if not effective_sid and self.voice_hint:
                payload["sessionConfig"] = {"systemPrompt": self.voice_hint}
            await ws.send(json.dumps(payload))
            try:
                async for raw in ws:
                    if self._llm_completed:
                        break
                    evt = json.loads(raw)
                    evt_type = evt.get("type", "")
                    if evt_type == "session_created":
                        # NeoMind assigned the real session id — capture for
                        # reuse on subsequent turns (same voice conversation).
                        sid = evt.get("sessionId")
                        if sid:
                            self._neomind_session_id = sid
                            logger.debug("captured NeoMind sessionId: %s", sid)
                        continue  # not a content event; don't yield
                    if evt_type == "Content":
                        text = evt.get("content", "")
                        if text == self.INTERRUPTED_MARKER:
                            continue  # filter post-cancel marker
                        yield LlmEvent(type="Content", text=text)
                    elif evt_type == "Thinking":
                        yield LlmEvent(type="Thinking")
                    elif evt_type == "ToolCallStart":
                        yield LlmEvent(type="ToolCallStart",
                                       tool_name=evt.get("toolName"))
                    elif evt_type == "ToolCallEnd":
                        yield LlmEvent(type="ToolCallEnd")
                    elif evt_type == "Progress":
                        yield LlmEvent(type="Progress",
                                       progress=evt.get("progress", 0.0))
                    elif evt_type == "end":
                        self._llm_completed = True
                        yield LlmEvent(type="end")
                        return
                    elif evt_type in ("Error", "error"):
                        yield LlmEvent(type="Error", text=evt.get("message", ""))
                        return
            finally:
                self._active_ws = None

    async def cancel(self, session_id: str) -> None:
        if self._llm_completed:
            return  # no-op
        if self._active_ws and not getattr(self._active_ws, "closed", True):
            # Cancel uses the same ChatRequest schema: `message == "__CANCEL__"`.
            # Prefer the captured NeoMind session id over the caller-supplied
            # `session_id` (which is `str(id(pipeline))`).
            effective_sid = self._neomind_session_id or session_id
            await self._active_ws.send(json.dumps({
                "message": "__CANCEL__",
                "sessionId": effective_sid,
            }))
            # Per spec Section 3.3: wait for lowercase "end" event with 500ms timeout.
            # NeoMind emits Content "\\n\\n[Interrupted]" then "end" after cancel.
            try:
                await asyncio.wait_for(self._wait_for_end_event(), timeout=0.5)
            except asyncio.TimeoutError:
                logger.warning("NeoMind cancel ack timeout (session=%s)", session_id)

    async def _wait_for_end_event(self) -> None:
        """Block until the active WS emits lowercase 'end' event."""
        if not self._active_ws:
            return
        async for raw in self._active_ws:
            evt = json.loads(raw)
            if evt.get("type") == "end":  # lowercase, verified
                return
            # Discard "[Interrupted]" Content events — do nothing


class NeoMindCapabilityLLM:
    """LLM backend that consumes NeoMind chat streaming via the platform's
    ChatStream capability instead of a direct authenticated WS to NeoMind.

    Eliminates the need for NEOMIND_TOKEN in the voice-assistant profile.
    Token-free routing is mediated by the Rust extension layer:

      Python (this client)
        ─── sends {"type":"chat_stream_request", message, session_id?} text frame
        ─── over the existing /ws connection to the voice-assistant Rust ext
        ↓
      Rust extension (CapabilityContext.invoke_capability("chat_stream", ...))
        ↓
      NeoMind host ChatStreamCapabilityProvider (SessionManager +
        EventBus broadcast of AgentStreamChunk)
        ↓
      Rust extension (global event handler → routes AgentStreamChunk
        events by session_id back to the originating pump's chat_chunks mpsc)
        ↓
      Python (this client consumes chat_chunk frames demultiplexed by the
        main ws_handler into self._chat_rx)

    Implements the LLMClient Protocol. Drop-in alternative to NeoMindWSClient;
    pick via profile: ``llm.type: neomind_capability``.

    Constructor takes the live WebSocket (the same FastAPI WebSocket the
    ws_handler owns) and a per-session ``asyncio.Queue`` into which the main
    receive loop pushes inbound ``chat_chunk`` / ``chat_stream_started`` /
    ``chat_stream_end`` / ``chat_stream_error`` text frames demultiplexed
    away from the existing transcript/stop/pong handling.
    """

    DEFAULT_VOICE_HINT = NeoMindWSClient.DEFAULT_VOICE_HINT

    def __init__(
        self,
        ws,
        chat_rx: "asyncio.Queue[dict]",
        voice_hint: str | None = None,
        timeout: float = 60.0,
    ):
        self.ws = ws
        self._chat_rx = chat_rx
        self.voice_hint = self.DEFAULT_VOICE_HINT if voice_hint is None else voice_hint
        self.timeout = timeout
        # NeoMind-assigned session id (captured from chat_stream_started).
        # Reused across turns in the same voice conversation so the LLM keeps
        # full history. Mirrors NeoMindWSClient._neomind_session_id semantics.
        self._neomind_session_id: str | None = None
        self._cancel_requested: bool = False

    async def stream(self, user_text: str, session_id: str) -> AsyncIterator[LlmEvent]:
        # session_id arg is the voice pipeline's Python id (str(id(pipeline))),
        # NOT a NeoMind session uuid. We carry our own captured neomind id.
        self._cancel_requested = False
        # User message is sent VERBATIM. The voice_hint ("short spoken
        # replies, no markdown, …") is no longer prepended to each user
        # message — it is injected ONCE at session creation as the LLM's
        # `system_prompt` via the Rust extension's chat_session_open
        # capability call (see lib.rs → handle_stream_chunk flow). The
        # LLM remembers the instruction for the lifetime of the session
        # without polluting every user turn. This requires PR1 of the
        # platform-side ChatRequest.sessionConfig / CreateSessionOptions
        # plumbing (NeoMind 0.9.1+).
        request: dict = {"type": "chat_stream_request", "message": user_text}
        if self._neomind_session_id:
            request["session_id"] = self._neomind_session_id
        else:
            # First turn in this voice conversation — also pass the hint
            # through so Rust can forward it to chat_session_open as
            # system_prompt. Subsequent turns omit the field entirely
            # (existing sessions ignore it anyway; saves bandwidth and
            # makes the wire trace obvious about which turn created the
            # session).
            if self.voice_hint:
                request["voice_hint"] = self.voice_hint

        # NOTE: No cross-turn drain loop. The platform's AgentStreamEnd is
        # the authoritative stream terminator and is delivered to Python
        # via the Rust extension's chat_stream_end WS frame. We always
        # consume that sentinel before returning (the `chat_stream_end`
        # branch below returns), so there is no leftover sentinel in the
        # queue to leak into the next turn. Removing this drain is a
        # deliberate correctness property — see Phase 1 of the ChatStream
        # refactor ("ChatStream Refactor: Persistent Session-Stream +
        # Direct Routing"). If you re-add a drain here you are masking a
        # bug in the terminator path; fix that instead.

        try:
            await self.ws.send_text(json.dumps(request, ensure_ascii=False))
        except Exception as e:
            yield LlmEvent(type="Error", text=f"send_text failed: {e}")
            return
        logger.info(
            "NeoMindCapabilityLLM.stream sent chat_stream_request: "
            "reuse_neomind_sid=%r, msg_len=%d",
            self._neomind_session_id, len(user_text),
        )

        while True:
            if self._cancel_requested:
                yield LlmEvent(type="end")
                return
            try:
                msg = await asyncio.wait_for(self._chat_rx.get(), timeout=self.timeout)
            except asyncio.TimeoutError:
                yield LlmEvent(type="Error", text="chat_stream timeout")
                return
            mtype = msg.get("type")
            logger.debug("NeoMindCapabilityLLM msg: type=%s, keys=%s", mtype, list(msg.keys()))
            if mtype == "chat_stream_started":
                sid = msg.get("session_id")
                if sid:
                    self._neomind_session_id = sid
                    logger.info("NeoMindCapabilityLLM captured session_id: %s", sid)
                else:
                    logger.info("NeoMindCapabilityLLM chat_stream_started without sid")
                continue
            if mtype == "chat_session_turn_started":
                # Phase 2: emitted by the Rust extension after each
                # chat_session_send. turn_id is also inside each chunk's
                # wrapper; this is just an early marker that the host has
                # accepted the send. Optional — not gating chunk flow.
                turn_id = msg.get("turn_id")
                logger.debug(
                    "NeoMindCapabilityLLM turn_started sid=%s turn_id=%s",
                    msg.get("session_id"), turn_id,
                )
                continue
            if mtype == "chat_stream_error":
                logger.warning("NeoMindCapabilityLLM chat_stream_error: %s", msg.get("error"))
                yield LlmEvent(type="Error", text=msg.get("error", "unknown"))
                return
            if mtype == "chat_stream_end":
                # Terminal sentinel from the Rust pump. If we already yielded
                # an LlmEvent(type="end") from a chunk "End", this is a no-op
                # duplicate — exit cleanly.
                logger.info("NeoMindCapabilityLLM chat_stream_end received")
                yield LlmEvent(type="end")
                return
            if mtype != "chat_chunk":
                continue
            chunk = msg.get("chunk") or {}
            ctype = chunk.get("type")
            if ctype == "Content":
                text = chunk.get("content", "")
                if text == NeoMindWSClient.INTERRUPTED_MARKER:
                    continue  # filter post-cancel marker, same as NeoMindWSClient
                yield LlmEvent(type="Content", text=text)
            elif ctype == "Thinking":
                yield LlmEvent(type="Thinking")
            elif ctype == "ToolCallStart":
                yield LlmEvent(type="ToolCallStart", tool_name=chunk.get("toolName"))
            elif ctype == "ToolCallEnd":
                yield LlmEvent(type="ToolCallEnd")
            elif ctype == "Progress":
                yield LlmEvent(type="Progress", progress=chunk.get("progress", 0.0))
            elif ctype in ("End", "end"):
                # Terminal AgentEvent — Rust also emits chat_stream_end after
                # this; we exit here to avoid a double "end". Accept both
                # casings: capability provider mirrors the WS handler which
                # emits lowercase "end" (sessions.rs:223), but legacy paths
                # and some tests use "End".
                yield LlmEvent(type="end")
                return
            elif ctype in ("Error", "error"):
                yield LlmEvent(type="Error", text=chunk.get("message", ""))
                return
            # Other event types (Heartbeat, Warning, Intent, Plan,
            # IntermediateEnd) are ignored — orchestrator doesn't act on them.

    async def cancel(self, session_id: str) -> None:
        self._cancel_requested = True
        # Unblock any in-flight stream() awaiting chat_rx.get(). Pushes a
        # terminal sentinel that the loop's next iteration will yield as 'end'
        # (cancel flag is checked at the top of the loop; this also covers
        # the case where stream() is between iterations).
        try:
            self._chat_rx.put_nowait({"type": "chat_stream_end"})
        except asyncio.QueueFull:
            pass  # bounded queue edge case; flag check still terminates
        if self._neomind_session_id:
            try:
                await self.ws.send_text(json.dumps({
                    "type": "chat_stream_cancel",
                    "session_id": self._neomind_session_id,
                }))
            except Exception as e:
                logger.warning("chat_stream_cancel send failed: %s", e)
