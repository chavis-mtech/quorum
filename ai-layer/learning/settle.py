"""Settlement — given the candles AFTER a long decision, did it win or lose?

This is the heart of "learning by itself": we replay the real high/low path the market printed
after each recorded decision and decide the outcome as an R-multiple (net of round-trip fees).

R-multiple = realized_reward / risk, where risk = entry − stop. So a clean stop-out ≈ −1R and
hitting a 2.5:1 target ≈ +2.5R. Fees are subtracted in price terms so the learned expectancy is
what the account would ACTUALLY have experienced on Bitkub (≈0.25% per side).

Conventions:
  • Long only (the bot only takes longs on spot).
  • If a single bar trades through BOTH stop and target, assume the STOP hit first (conservative —
    never flatter the strategy than reality might be).
  • "expired": if neither level is hit within `max_bars`, mark-to-market at the last close so the
    decision still contributes a (small) learning signal instead of hanging open forever.
"""
from __future__ import annotations

from typing import Any

DEFAULT_FEE_PER_SIDE = 0.0025  # Bitkub spot taker fee ≈ 0.25% per side


def _net_r(entry: float, exit_price: float, risk: float, fee_per_side: float) -> float:
    fee_cost = (entry + exit_price) * fee_per_side  # round-trip fee in price units
    net_reward = (exit_price - entry) - fee_cost
    return net_reward / risk if risk > 0 else 0.0


def settle_long(
    entry: float,
    target: float,
    stop: float,
    bars: list[dict[str, float]],
    *,
    max_bars: int = 48,
    fee_per_side: float = DEFAULT_FEE_PER_SIDE,
) -> dict[str, Any]:
    """Replay `bars` (chronological, AFTER the decision) for a long entry/target/stop.

    Returns {"status": "open"|"win"|"loss"|"expired", "exit": float|None,
             "r": float|None, "bars_held": int}.
    """
    risk = entry - stop
    if risk <= 0 or entry <= 0:
        return {"status": "invalid", "exit": None, "r": None, "bars_held": 0}

    held = 0
    last_close = entry
    for c in bars[:max_bars]:
        held += 1
        hi = float(c.get("high", 0.0))
        lo = float(c.get("low", 0.0))
        last_close = float(c.get("close", last_close)) or last_close
        hit_stop = lo > 0 and lo <= stop
        hit_target = hi > 0 and hi >= target
        if hit_stop:  # checked first → conservative when a bar straddles both
            return {"status": "loss", "exit": stop,
                    "r": round(_net_r(entry, stop, risk, fee_per_side), 4), "bars_held": held}
        if hit_target:
            return {"status": "win", "exit": target,
                    "r": round(_net_r(entry, target, risk, fee_per_side), 4), "bars_held": held}

    # not resolved
    if len(bars) >= max_bars:
        return {"status": "expired", "exit": last_close,
                "r": round(_net_r(entry, last_close, risk, fee_per_side), 4), "bars_held": held}
    return {"status": "open", "exit": None, "r": None, "bars_held": held}
