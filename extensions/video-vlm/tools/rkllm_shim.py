#!/usr/bin/env python3
"""rkllm shim: translate LFM bracket tool calls into NeoMind Hermes format.

Sits between NeoMind and rkllm3-server (upstream 127.0.0.1:8080), listening
on 127.0.0.1:8081. The 2.6B model emits tool calls in its trained bracket
format —  [web_fetch(url='...', format='text')]  — which NeoMind's text
tool parser does not understand. This shim rewrites them (in both streamed
deltas and full JSON responses) into the Hermes format NeoMind parses:

    <function name="web_fetch">
    <param name="url">https://...</param>
    <param name="format">text</param>
    </function>

Everything else (models list, reasoning_content, native tool_calls, stats)
passes through untouched.

Usage: python3 rkllm_shim.py   (listens on 127.0.0.1:8081)
"""
import json
import re
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import urllib.request

UPSTREAM = "http://127.0.0.1:8080"
PORT = 8081

# A potential bracket call starts like: [name(
START_RE = re.compile(r"\[([a-zA-Z_][\w.]*)\(")
MAX_HOLD = 8192
# A tail that could still grow into a call start: "[" or "[" + partial name
PARTIAL_RE = re.compile(r"^\[[a-zA-Z_][\w.]*$|^\[$")  # bytes we'll buffer while deciding



TOOL_NAME_RE = re.compile(r"^\*\*([a-zA-Z_][\w:.-]*)\*\*:\s*(.+)$", re.M)


def extract_tools(messages):
    """Pull tool definitions out of NeoMind's system-prompt tool section."""
    sysp = ""
    for m in messages:
        if isinstance(m, dict) and m.get("role") == "system":
            c = m.get("content")
            sysp = c if isinstance(c, str) else ""
            break
    if not sysp or "Available Tools" not in sysp:
        return None
    tools = []
    for m in TOOL_NAME_RE.finditer(sysp):
        name = m.group(1)
        desc = m.group(2).strip()[:200]
        # collect the Parameters block that may follow (extension tools)
        props = {}
        tail = sysp[m.end(): m.end() + 1200]
        if "Parameters:" in tail[:400]:
            for pm in re.finditer(r"^\s+- `([\w]+)`:\s*(.*)$", tail, re.M):
                props[pm.group(1)] = {"type": "string", "description": pm.group(2).strip()[:120]}
                if len(props) >= 8:
                    break
        schema = {"type": "object", "properties": props}
        if not props:
            schema["additionalProperties"] = True
        tools.append({"type": "function", "function": {"name": name, "description": desc, "parameters": schema}})
    return tools or None


def tool_calls_to_text(acc):
    """Accumulated tool_calls -> NeoMind JSON-array text format."""
    if not acc:
        return ""
    calls = []
    for i in sorted(acc.keys()):
        c = acc[i]
        if not c.get("name"):
            continue
        try:
            args = json.loads(c.get("args") or "{}")
        except Exception:
            args = {"_raw": c.get("args", "")}
        calls.append({"name": c["name"], "arguments": args})
    if not calls:
        return ""
    return "\n" + json.dumps(calls, ensure_ascii=False)

def split_args(s):
    """Split 'k=v, k2=v2' on top-level commas, quote aware."""
    parts, depth, quote, cur, i = [], 0, None, "", 0
    while i < len(s):
        c = s[i]
        if quote:
            cur += c
            if c == "\\" and i + 1 < len(s):
                cur += s[i + 1]
                i += 2
                continue
            if c == quote:
                quote = None
        elif c in "'\"":
            quote = c
            cur += c
        elif c in "([{":
            depth += 1
            cur += c
        elif c in ")]}":
            depth -= 1
            cur += c
        elif c == "," and depth == 0:
            parts.append(cur.strip())
            cur = ""
        else:
            cur += c
        i += 1
    if cur.strip():
        parts.append(cur.strip())
    return parts


def parse_one_call(text):
    """Parse 'name(k=v, ...)' -> (name, args_dict) or None."""
    text = text.strip()
    m = re.match(r"^([a-zA-Z_][\w.]*)\((.*)\)$", text, re.S)
    if not m:
        return None
    name, args_src = m.group(1), m.group(2)
    args = {}
    for p in split_args(args_src):
        if "=" not in p:
            continue
        k, _, v = p.partition("=")
        k, v = k.strip(), v.strip()
        if len(v) >= 2 and v[0] == v[-1] and v[0] in "'\\"":
            val = v[1:-1].replace("\\'", "'").replace('\\"', '"')
        else:
            try:
                val = json.loads(v)
            except Exception:
                val = v
        args[k] = val
    return name, args


def translate_group(name, args_src):
    """Translate the collected bracket group (may hold parallel calls).

    '[a(x=1), b(y=2)]' -> JSON array with both calls; single call likewise.
    """
    pieces = [name + "(" + args_src + ")"]
    # split parallel calls on top-level '), ' boundaries
    parts, depth, quote, cur = [], 0, None, ""
    for ch in args_src:
        if quote:
            cur += ch
            if ch == quote:
                quote = None
        elif ch in "'\\"":
            quote = ch; cur += ch
        elif ch in "([{":
            depth += 1; cur += ch
        elif ch in ")]}":
            depth -= 1; cur += ch
        elif ch == "," and depth == 0:
            parts.append(cur); cur = ""
        else:
            cur += ch
    if cur.strip():
        parts.append(cur)
    if len(parts) > 1:
        pieces = [p.strip() + ")" if p.strip().endswith(")") and not p.strip().endswith("()") else p.strip() + "()" for p in parts]
    calls = []
    for p in pieces:
        r = parse_one_call(p)
        if r:
            calls.append({"name": r[0], "arguments": r[1]})
    if not calls:
        return "[%s(%s)]" % (name, args_src)
    return json.dumps(calls, ensure_ascii=False)


def rewrite_text(text, state):
    """Stateful bracket-call -> Hermes rewrite for streaming content.

    `state` is a dict with 'hold' (bytes buffered while a call may be in
    progress). Returns the text safe to emit now.
    """
    buf = state.get("hold", "") + text
    out = []
    while buf:
        if state.get("in_call"):
            # Look for the end of the call: quote-aware scan for ')]'
            # (quote state persists across chunks via state["quote"])
            i, quote = 0, state.get("quote")
            while i < len(buf):
                c = buf[i]
                if quote:
                    if c == "\\":
                        i += 2
                        continue
                    if c == quote:
                        quote = None
                elif c in "'\"":
                    quote = c
                elif buf.startswith(")]", i):
                    m = START_RE.match(state["call_head"] or "")
                    name = m.group(1) if m else "call"
                    args_src = state["call_args"] + buf[:i]
                    out.append(translate_group(name, args_src))
                    buf = buf[i + 2:]
                    state["in_call"] = False
                    state["call_head"] = ""
                    state["call_args"] = ""
                    state["quote"] = None
                    break
                elif c == "[" and not quote and buf.startswith("[", i):
                    # nested bracket (json arg) — keep scanning
                    pass
                i += 1
            else:
                # call not complete yet — keep buffering (and keep quote state).
                # A trailing ')' may be the first half of ')]' split across
                # chunk boundaries — hold it back for the next scan.
                state["quote"] = quote
                keep = 1 if (buf.endswith(")") and not quote) else 0
                state["call_args"] += buf[: len(buf) - keep] if keep else buf
                state["hold"] = buf[-keep:] if keep else ""
                if len(state["call_args"]) > MAX_HOLD:  # bail: flush verbatim
                    out.append(state["call_head"] + state["call_args"])
                    state.update(in_call=False, call_head="", call_args="")
                buf = ""
                break
            continue

        m = START_RE.search(buf)
        if not m:
            # Hold any trailing partial call-start ('[' alone, or '[' + partial
            # name without '(' yet) — it may complete on the next chunk.
            idx = buf.rfind("[")
            if idx != -1 and PARTIAL_RE.search(buf[idx:]):
                out.append(buf[:idx])
                state["hold"] = buf[idx:]
            else:
                out.append(buf)
                state["hold"] = ""
            return "".join(out)
        out.append(buf[: m.start()])
        head = m.group(0)  # '[name('
        rest = buf[m.end():]
        state.update(in_call=True, call_head=head, call_args="", hold="")
        buf = rest
    return "".join(out)


def flush_hold(state):
    """At stream end, release anything still held."""
    h = state.get("hold", "") or ""
    if state.get("in_call"):
        h = state["call_head"] + state.get("call_args", "") + h
    state.update(hold="", in_call=False, call_head="", call_args="", quote=None)
    return h


class Shim(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        print("[shim] %s %s" % (self.command, self.path), flush=True)

    def _proxy(self, body=None):
        url = UPSTREAM + self.path
        req = urllib.request.Request(url, data=body, method=self.command)
        for k, v in self.headers.items():
            if k.lower() in ("host", "content-length", "connection", "transfer-encoding"):
                continue
            req.add_header(k, v)
        if body is not None:
            req.add_header("Content-Length", str(len(body)))
        try:
            resp = urllib.request.urlopen(req, timeout=300)
        except urllib.error.HTTPError as e:
            resp = e
        return resp

    def do_GET(self):
        resp = self._proxy()
        data = resp.read()
        self.send_response(resp.status if hasattr(resp, "status") else resp.code)
        ct = resp.headers.get("Content-Type", "application/json")
        self.send_header("Content-Type", ct)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        n = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(n) if n else b""

        if not self.path.startswith("/v1/chat/completions"):
            resp = self._proxy(body)
            data = resp.read()
            self.send_response(resp.status if hasattr(resp, "status") else resp.code)
            self.send_header("Content-Type", resp.headers.get("Content-Type", "application/json"))
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return

        try:
            payload = json.loads(body)
        except Exception:
            payload = {}
        streaming = bool(payload.get("stream"))

        # Inject tools extracted from the system prompt so the server's
        # NATIVE bracket-call parser activates (it only parses when the
        # request carries tools, and only for declared names).
        if payload and not payload.get("tools"):
            tools = extract_tools(payload.get("messages") or [])
            if tools:
                payload["tools"] = tools
                body = json.dumps(payload, ensure_ascii=False).encode()

        resp = self._proxy(body)

        self.send_response(resp.status if hasattr(resp, "status") else resp.code)
        for k, v in resp.headers.items():
            if k.lower() in ("transfer-encoding", "content-length", "connection"):
                continue
            self.send_header(k, v)

        if not streaming:
            data = resp.read()
            try:
                d = json.loads(data)
                for ch in d.get("choices", []):
                    msg = ch.get("message") or {}
                    tcs = msg.get("tool_calls") or []
                    if tcs:
                        st = {}
                        for i, tc in enumerate(tcs):
                            fn = tc.get("function") or {}
                            st[i] = {"name": fn.get("name") or "",
                                     "args": fn.get("arguments") or ""}
                        txt = tool_calls_to_text(st)
                        msg["content"] = ((msg.get("content") or "") + txt) or None
                    else:
                        c = msg.get("content")
                        if isinstance(c, str) and "[" in c:
                            st = {}
                            msg["content"] = rewrite_text(c, st) + flush_hold(st)
                data = json.dumps(d, ensure_ascii=False).encode()
            except Exception:
                pass
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return

        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()
        state = {}

        def chunk(b):
            if not b:
                return
            self.wfile.write(b"%x\r\n" % len(b) + b + b"\r\n")

        try:
            for raw in resp:
                if not raw:
                    continue
                if not raw.startswith(b"data:"):
                    chunk(raw)
                    continue
                payload_str = raw[5:].strip()
                if not payload_str or payload_str == b"[DONE]":
                    if payload_str == b"[DONE]":
                        chunk(b"data: [DONE]\n\n")
                    else:
                        chunk(raw)
                    continue
                try:
                    ev = json.loads(payload_str)
                except Exception:
                    chunk(raw)
                    continue
                for ch in ev.get("choices", []):
                    delta = ch.get("delta") or {}
                    c = delta.get("content")
                    if isinstance(c, str) and c:
                        delta["content"] = rewrite_text(c, state)
                    for tc in delta.get("tool_calls") or []:
                        idx = tc.get("index", 0)
                        acc = state.setdefault("tc", {}).setdefault(idx, {"name": "", "args": ""})
                        fn = tc.get("function") or {}
                        if fn.get("name"):
                            acc["name"] = (acc.get("name") or "") + fn["name"]
                        if fn.get("arguments"):
                            acc["args"] += fn["arguments"]
                chunk(b"data: " + json.dumps(ev, ensure_ascii=False).encode() + b"\n\n")
            tail = flush_hold(state) + tool_calls_to_text(state.get("tc") or {})
            if tail:
                fin = {"choices": [{"delta": {"content": tail}}]}
                chunk(b"data: " + json.dumps(fin, ensure_ascii=False).encode() + b"\n\n")
            chunk(b"data: [DONE]\n\n")
            self.wfile.write(b"0\r\n\r\n")
        except (BrokenPipeError, ConnectionResetError):
            pass


if __name__ == "__main__":
    srv = ThreadingHTTPServer(("127.0.0.1", PORT), Shim)
    print("[shim] listening on 127.0.0.1:%d -> %s" % (PORT, UPSTREAM), flush=True)
    srv.serve_forever()
