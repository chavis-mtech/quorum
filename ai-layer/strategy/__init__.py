"""strategy — deterministic, explainable decision rules applied to every verdict.

Kept separate from judge.py (which is LLM/provider plumbing) so the *trading discipline*
is small, pure, and unit-testable on its own:

  • entry_discipline — anti-chase: an over-extended market BUY becomes a pullback limit / HOLD.
  • trend_gate       — anti "falling-knife": a BUY into a confirmed downtrend is blocked unless a
                        reversal is confirmed; also scores conviction vs reversal-risk and sizes by it.

Both take plain dicts (verdict, ctx, consensus) and return a verdict dict — no I/O, no globals.
"""
from __future__ import annotations

from .entry_discipline import apply_entry_discipline
from .trend_gate import apply_trend_gate, assess_trend

__all__ = ["apply_entry_discipline", "apply_trend_gate", "assess_trend"]
