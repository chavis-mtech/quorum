"""Entry discipline — regime-aware anti-chase guard (moved out of judge.py).

Entry STYLE must match the market regime — do not blindly demand a cheaper price:
  • trending   → momentum IS the signal; ride strength / breakouts. Enter at market unless the
                 move is truly parabolic (blow-off). Demanding a pullback here just misses it.
  • weak-trend → moderate: enter at market on a clean setup, pull back if stretched.
  • ranging    → the edge is mean-reversion; buy a dip toward support, never chase a local high.

When a market BUY is judged over-extended FOR ITS REGIME, it is converted to a small pullback
LIMIT (or HOLD if no safe pullback keeps the reward:risk). Thresholds are deterministic and
computed from real structure — they do NOT depend on the (sometimes unreliable) local LLM.

Per regime: (min_room_to_resistance, max_dist_above_support, hot_momentum_5, overbought_rsi).
A market BUY is "extended" if room-to-resistance is below / distance-above-support, 5-bar
momentum, or RSI is at-or-above the regime's bar.
"""
from __future__ import annotations

from typing import Any

_EXT_THRESHOLDS: dict[str, tuple[float, float, float, float]] = {
    "trending":   (0.010, 0.30, 0.18, 82.0),  # lenient — strong trend, let the entry run
    "weak-trend": (0.025, 0.15, 0.10, 74.0),  # moderate
    "ranging":    (0.040, 0.08, 0.06, 68.0),  # strict — only buy pullbacks to support
    "unknown":    (0.030, 0.12, 0.08, 72.0),  # neutral default
}
_EXT_PULLBACK_ATR = 0.5          # aim the limit ~0.5*ATR below market
_EXT_PULLBACK_PCT = 0.02         # fallback pullback when ATR is unavailable (2% below market)
_EXT_MIN_PULLBACK_PCT = 0.015    # the limit must sit at least 1.5% below market
_EXT_MAX_PULLBACK_PCT = 0.045    # ...but within 4.5% (Rust rejects pending entries >5% away)


def apply_entry_discipline(v: dict[str, Any], ctx: dict[str, Any] | None) -> dict[str, Any]:
    """Regime-aware anti-chase guard applied to EVERY final verdict (LLM and rule-based alike).

    A market BUY that is over-extended FOR ITS REGIME is converted to a pullback LIMIT entry —
    or HOLD when no safe pullback keeps the reward:risk. In a strong trend the bar for "extended"
    is high (ride the move); in a range it is low (only buy dips). This stops the bot from both
    chasing blow-off tops AND from missing clean trend entries by always waiting for a dip.
    """
    if str(v.get("action")) != "BUY" or str(v.get("entry_type")) != "market":
        return v  # only market BUYs can chase; limit/none/HOLD/SELL are left untouched
    s = (ctx or {}).get("market_structure") or {}
    if s.get("quality") != "ok":
        return v
    price = float((ctx or {}).get("last_price") or v.get("entry_price") or 0.0)
    if price <= 0:
        return v

    regime = str((ctx or {}).get("regime") or "unknown").lower()
    min_room, max_dist, hot_mom, ob_rsi = _EXT_THRESHOLDS.get(
        regime, _EXT_THRESHOLDS["unknown"]
    )

    atr = float(s.get("atr_14") or 0.0)
    sup = float(s.get("support_20") or 0.0)
    room = float(s.get("room_to_resistance_20") or 0.0)      # (resistance/price - 1)
    dist_sup = float(s.get("distance_to_support_20") or 0.0)  # (price/support - 1)
    mom5 = float(s.get("momentum_5") or 0.0)
    rsi = float(s.get("rsi") or 0.0)

    reasons: list[str] = []
    if 0.0 < room < min_room:
        reasons.append(f"only {room * 100:.1f}% room to resistance")
    if dist_sup > max_dist:
        reasons.append(f"{dist_sup * 100:.1f}% above support")
    if mom5 > hot_mom:
        reasons.append(f"5-bar momentum +{mom5 * 100:.1f}%")
    if rsi >= ob_rsi:
        reasons.append(f"RSI {rsi:.0f} overbought")
    if not reasons:
        return v  # clean entry for this regime → take it at market (ride strength)

    # Build a sane pullback limit: aim ~0.5*ATR below market, pulled at least 1.5% below
    # market so it is a real dip-entry, then floored so it never sits below 20-bar support
    # and never more than 4.5% below market (Rust rejects pending entries >5% away).
    pullback = price - _EXT_PULLBACK_ATR * atr if atr > 0 else price * (1 - _EXT_PULLBACK_PCT)
    pullback = min(pullback, price * (1 - _EXT_MIN_PULLBACK_PCT))
    floor = price * (1 - _EXT_MAX_PULLBACK_PCT)
    if sup > 0:
        floor = max(floor, sup * 1.002)  # support floor wins over the min-pullback target
    pullback = max(pullback, floor)

    stop = float(v.get("stop_price") or 0.0)
    target = float(v.get("target_price") or 0.0)
    note = f"{regime}: " + "; ".join(reasons)
    # If the pullback would break the stop or overshoot the target, there is no clean
    # entry left → stand aside rather than chase.
    if pullback <= 0 or (stop > 0 and pullback <= stop) or (target > 0 and target <= pullback):
        v["action"] = "HOLD"
        v["entry_type"] = "none"
        v["entry_price"] = 0.0
        v["reasoning"] = (str(v.get("reasoning", "")) +
                          f" | entry-discipline: over-extended ({note}); no safe pullback → HOLD")[:4000]
        return v
    v["entry_type"] = "limit"
    v["entry_price"] = round(pullback, 8)
    v["reasoning"] = (str(v.get("reasoning", "")) +
                      f" | entry-discipline: over-extended ({note}) → wait for pullback to {pullback:.6g}")[:4000]
    return v
