"""Sidecar server — JSON over stdio (called by Rust core)

Simple protocol: receive 1 line = 1 JSON request, reply 1 line = 1 JSON response
  request : {"id": 1, "method": "analyze", "params": {"symbol": "BTC"}}
  response: {"id": 1, "ok": true, "result": {...}}

This requires no additional dependencies (suitable for Tauri sidecar via stdin/stdout)
Can be upgraded to gRPC later without affecting the pipeline
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from config_loader import load_config
from pipeline import analyze_symbol

_CFG = load_config()


def _handle(req: dict) -> dict:
    rid = req.get("id")
    method = req.get("method")
    params = req.get("params") or {}
    try:
        if method == "ping":
            return {"id": rid, "ok": True, "result": "pong"}
        if method == "config":
            return {"id": rid, "ok": True, "result": _CFG}
        if method == "analyze":
            symbol = params.get("symbol") or _CFG["market"]["symbols"][0]
            return {"id": rid, "ok": True,
                    "result": analyze_symbol(symbol, _CFG,
                                             judge_override=params.get("judge_override"))}
        return {"id": rid, "ok": False, "error": f"unknown method: {method}"}
    except Exception as exc:  # don't let the server die from a single bad request
        return {"id": rid, "ok": False, "error": str(exc)}


def main() -> int:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError as exc:
            sys.stdout.write(json.dumps({"ok": False, "error": f"bad json: {exc}"}) + "\n")
            sys.stdout.flush()
            continue
        resp = _handle(req)
        sys.stdout.write(json.dumps(resp, ensure_ascii=False) + "\n")
        sys.stdout.flush()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
