#!/usr/bin/env python3
"""Mock VLM endpoint for pipeline verification on the RK board.

Responds to OpenAI-style /v1/chat/completions with a canned description.
Logs every request so we can confirm frames are flowing through the
video-vlm-v2 extension (VLM_VIDEO_ENDPOINT points here instead of the
real rkllm3-server, which is currently hanging on the card).

Usage: python3 mock_vlm.py  (listens on 127.0.0.1:9898)
"""
import json
import time
from http.server import BaseHTTPRequestHandler, HTTPServer


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(n)
        try:
            req = json.loads(body)
            msgs = req.get("messages", [])
            has_img = any(
                isinstance(m.get("content"), list) for m in msgs
            )
        except Exception:
            has_img = False
        print(
            "[%s] VLM request len=%d img=%s"
            % (time.strftime("%H:%M:%S"), n, "Y" if has_img else "N"),
            flush=True,
        )
        resp = json.dumps(
            {
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "【MOCK】画面中有视频测试场景，人物走动。",
                        }
                    }
                ]
            }
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(resp)))
        self.end_headers()
        self.wfile.write(resp)

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    HTTPServer(("127.0.0.1", 9898), Handler).serve_forever()
