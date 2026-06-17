"""HTTP server for the AI layer — allows Rust backend to call via HTTP (stdlib only)

endpoints:
  GET  /health           → {"ok": true}
  POST /analyze          → body {"symbol":"BTC"} → full Analysis JSON

Run:
  python3 ai-layer/http_server.py            # listens on 127.0.0.1:8765
  AI_HOST=0.0.0.0 AI_PORT=8765 python3 ...   # override host/port via env
"""
from __future__ import annotations

import json
import os
import socket
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from config_loader import load_config
from pipeline import analyze_symbol, analyze_symbol_stream

_CFG = load_config()


class Handler(BaseHTTPRequestHandler):
    def _send(self, code: int, payload: dict) -> None:
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        try:
            self.send_response(code)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError, socket.timeout):
            # client closed the connection early (e.g. timeout during analyze) — not a server fault
            return

    def do_GET(self) -> None:  # noqa: N802
        if self.path.startswith("/health"):
            self._send(200, {"ok": True, "service": "quorum-ai"})
        else:
            self._send(404, {"error": "not found"})

    def _stream_ndjson(self, gen) -> None:
        """Stream events line by line (NDJSON) — flush each line so the client sees real-time updates"""
        try:
            self.send_response(200)
            self.send_header("Content-Type", "application/x-ndjson; charset=utf-8")
            self.send_header("Cache-Control", "no-cache")
            self.send_header("Connection", "close")
            self.end_headers()
            for ev in gen:
                line = (json.dumps(ev, ensure_ascii=False) + "\n").encode("utf-8")
                self.wfile.write(line)
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, socket.timeout):
            return
        except Exception as exc:
            try:
                err = (json.dumps({"type": "error", "error": str(exc)}) + "\n").encode("utf-8")
                self.wfile.write(err)
                self.wfile.flush()
            except Exception:
                pass

    def do_POST(self) -> None:  # noqa: N802
        if not self.path.startswith("/analyze"):
            self._send(404, {"error": "not found"})
            return
        try:
            length = int(self.headers.get("Content-Length", 0))
            req = json.loads(self.rfile.read(length) or b"{}")
            symbol = (req.get("symbol") or _CFG["market"]["symbols"][0]).upper()
        except (BrokenPipeError, ConnectionResetError, socket.timeout):
            return
        except Exception as exc:
            self._send(400, {"error": str(exc)})
            return

        # streaming: shows thinking + progress percentage while processing
        if self.path.startswith("/analyze/stream"):
            self._stream_ndjson(
                analyze_symbol_stream(symbol, _CFG, judge_override=req.get("judge_override"))
            )
            return

        # non-streaming (original)
        try:
            result = analyze_symbol(symbol, _CFG, judge_override=req.get("judge_override"))
            self._send(200, result)
        except (BrokenPipeError, ConnectionResetError, socket.timeout):
            return
        except Exception as exc:
            self._send(500, {"error": str(exc)})

    def log_message(self, fmt: str, *args) -> None:  # silence default log output
        return


def main() -> int:
    host = os.environ.get("AI_HOST", "127.0.0.1")
    port = int(os.environ.get("AI_PORT", "8765"))
    server = ThreadingHTTPServer((host, port), Handler)
    print(f"[quorum-ai] listening on http://{host}:{port}  (agents ready)")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n[quorum-ai] server stopped")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
