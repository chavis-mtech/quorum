"""Trace — records "what the AI is thinking" step by step, displayed on the UI as a timeline

Each step: stage, title, detail, status, data, elapsed_ms
Used in the pipeline so users can see "what each phase is thinking/doing"
"""
from __future__ import annotations

import time
from typing import Any


class Trace:
    def __init__(self) -> None:
        self._steps: list[dict[str, Any]] = []
        self._t0 = time.time()

    def add(self, stage: str, title: str, detail: str = "",
            status: str = "done", data: dict[str, Any] | None = None) -> None:
        self._steps.append({
            "seq": len(self._steps) + 1,
            "stage": stage,          # data | news | web | agent | consensus | judge
            "title": title,
            "detail": detail,
            "status": status,        # done | warn | error | thinking
            "data": data or {},
            "elapsed_ms": int((time.time() - self._t0) * 1000),
        })

    def to_list(self) -> list[dict[str, Any]]:
        return self._steps
