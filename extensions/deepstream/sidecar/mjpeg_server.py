#!/usr/bin/env python3
"""Lightweight MJPEG streaming server for DeepStream preview tiles.

Converts DeepStream's annotated RTSP output streams into Motion-JPEG over HTTP
so they can be displayed as <img> sources in the NeoMind dashboard grid.

Usage:
    python3 mjpeg_server.py [--port 8090]

The server spawns one ffmpeg process per connected client. Each ffmpeg reads
the DeepStream RTSP output (rtsp://127.0.0.1:8554/ds/<stream_id>) and pipes
MJPEG frames to stdout.

Requires ffmpeg to be installed on the host (the Jetson).
"""

import http.server
import subprocess
import re
import sys
import argparse

PORT = 8090
RTSP_BASE = "rtsp://127.0.0.1:8554/ds"
FPS = 15
WIDTH = 960
HEIGHT = 540

STREAM_ID_RE = re.compile(r'^[A-Za-z0-9_\-]+$')
BOUNDARY = "mjpegboundary"


def read_jpeg_frames(proc):
    buf = b''
    SOI = b'\xff\xd8'
    EOI = b'\xff\xd9'
    while True:
        chunk = proc.stdout.read1(4096)
        if not chunk:
            start = buf.find(SOI)
            if start != -1:
                end = buf.find(EOI, start + 2)
                if end != -1:
                    yield buf[start:end + 2]
            return
        buf += chunk
        while True:
            start = buf.find(SOI)
            if start == -1:
                buf = b''
                break
            end = buf.find(EOI, start + 2)
            if end == -1:
                buf = buf[start:]
                break
            yield buf[start:end + 2]
            buf = buf[end + 2:]


class MJPEGHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        parts = self.path.strip('/').split('/')
        if len(parts) != 2 or parts[0] != 'mjpeg':
            self.send_error(404, "Not found")
            return

        stream_id = parts[1]
        if not STREAM_ID_RE.match(stream_id):
            self.send_error(400, "Invalid stream ID")
            return

        rtsp_url = f"{RTSP_BASE}/{stream_id}"

        cmd = [
            "ffmpeg",
            "-rtsp_transport", "tcp",
            "-analyzeduration", "0",
            "-probesize", "512",
            "-i", rtsp_url,
            "-vf", f"scale={WIDTH}:{HEIGHT}",
            "-r", str(FPS),
            "-q:v", "2",
            "-f", "image2pipe",
            "-vcodec", "mjpeg",
            "-",
        ]

        try:
            proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=sys.stderr)
        except Exception as e:
            self.send_error(500, f"ffmpeg spawn failed: {e}")
            return

        self.send_response(200)
        self.send_header('Content-Type', f'multipart/x-mixed-replace; boundary={BOUNDARY}')
        self.send_header('Cache-Control', 'no-cache, private')
        self.send_header('Connection', 'close')
        self.end_headers()

        try:
            for jpeg in read_jpeg_frames(proc):
                frame = (
                    f'--{BOUNDARY}\r\n'.encode() +
                    b'Content-Type: image/jpeg\r\n' +
                    f'Content-Length: {len(jpeg)}\r\n'.encode() +
                    b'\r\n' +
                    jpeg +
                    b'\r\n'
                )
                self.wfile.write(frame)
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass
        except Exception as e:
            print(f"[mjpeg] error: {e}", file=sys.stderr)
        finally:
            proc.kill()
            proc.wait()

    def log_message(self, format, *args):
        pass


class ThreadedHTTPServer(http.server.ThreadingHTTPServer):
    daemon_threads = True


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='MJPEG preview server for DeepStream')
    parser.add_argument('--port', type=int, default=PORT)
    parser.add_argument('--rtsp-base', default=RTSP_BASE,
                        help='Base RTSP URL for DeepStream output (default: rtsp://127.0.0.1:8554/ds)')
    args = parser.parse_args()

    if args.rtsp_base:
        RTSP_BASE = args.rtsp_base.rstrip('/')

    server = ThreadedHTTPServer(('0.0.0.0', args.port), MJPEGHandler)
    print(f"MJPEG server listening on :{args.port} ({WIDTH}x{HEIGHT} @{FPS}fps)", flush=True)
    server.serve_forever()
