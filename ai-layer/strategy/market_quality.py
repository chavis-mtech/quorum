"""Market-quality + fee-edge guard — refuse to trade un-winnable conditions.

Two deterministic reasons to NOT take an otherwise-valid long, both common causes of the
"every order loses" bleed on a spot bot:

  1) DEAD/CHOP MARKET — when the market is going nowhere (very low Efficiency Ratio + low ADX)
     or barely moving at all (tiny ATR%), entries get chopped up and the round-trip fee eats the
     rest. There is simply no edge to harvest, so we stand aside (and shadow-track to confirm).

  2) FEE-UNVIABLE TARGET — Bitkub charges ≈0.25% per side (≈0.5% round trip). If the planned
     target is so close that the move barely clears fees, the expected value after costs is
     negative even when the directional read is right. We require the target to clear a minimum
     net move AND keep a real reward:risk AFTER fees.

These are checks, not rewrites — they return ok + a reason; the caller converts a failed BUY to
HOLD (and records a shadow trade so the learner can verify the guard was correct).
"""
from __future__ import annotations

from typing import Any

DEFAULTS = {
    "fee_per_side": 0.0025,    # Bitkub spot taker ≈ 0.25%
    "min_target_move": 0.012,  # target must be ≥1.2% above entry (clears ~0.5% round trip + slippage)
    "min_net_rr": 1.2,         # reward:risk AFTER fees must stay ≥ this
    "chop_er": 0.12,           # Efficiency Ratio below this...
    "chop_adx": 18.0,          # ...and ADX below this = pure chop → no trade
    "flat_atr_pct": 0.003,     # ATR < 0.3% of price = market not moving → fees dominate
    "min_liquidity_thb": 50000.0,  # median THB traded per bar; below this the book is too thin
    "max_atr_pct": 0.025,      # ATR > 2.5% per bar = too wild for a fixed stop → noise stops you out
}


def assess_quality(
    structure: dict[str, Any],
    regime: str,
    entry: float,
    target: float,
    stop: float,
    cfg: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Return {"ok": bool, "reason": str, "checks": {...}} for a prospective long BUY."""
    c = {**DEFAULTS, **(cfg or {})}
    s = structure or {}
    er = float(s.get("efficiency_ratio") or 0.0)
    adx = float(s.get("adx") or 0.0)
    atr_pct = float(s.get("atr_pct") or 0.0)
    liq = float(s.get("median_thb_vol") or 0.0)
    fee = float(c["fee_per_side"])

    checks = {"er": er, "adx": adx, "atr_pct": atr_pct, "liquidity_thb": liq}

    # 1) liquidity — the #1 cause of heavy losses: thin coins slip through the stop and our own
    #    order moves the book. Refuse to enter a coin that trades too little per bar.
    if 0.0 < liq < c["min_liquidity_thb"]:
        return {"ok": False, "checks": checks,
                "reason": f"thin market: ~{liq:,.0f} THB/bar < {c['min_liquidity_thb']:,.0f} — "
                          f"order book too illiquid (stop will slip, our order moves price)"}

    # 2) too volatile — a coin swinging more than max_atr_pct per bar gets knifed out of a fixed
    #    stop by ordinary noise (e.g. the AERO/EIGEN-type −10% stop-slips).
    if atr_pct > c["max_atr_pct"]:
        return {"ok": False, "checks": checks,
                "reason": f"too volatile: ATR {atr_pct*100:.1f}%/bar > {c['max_atr_pct']*100:.1f}% — "
                          f"noise will knife a fixed stop"}

    # 3) dead / chop market
    if atr_pct > 0.0 and atr_pct < c["flat_atr_pct"]:
        return {"ok": False, "checks": checks,
                "reason": f"flat market: ATR {atr_pct*100:.2f}% < {c['flat_atr_pct']*100:.1f}% — "
                          f"too little movement to clear fees"}
    if er > 0.0 and er < c["chop_er"] and adx > 0.0 and adx < c["chop_adx"]:
        return {"ok": False, "checks": checks,
                "reason": f"chop: ER {er:.2f} & ADX {adx:.0f} both weak — no directional edge"}

    # 2) fee-unviable target (only checkable when we have real levels)
    if entry > 0 and target > 0 and stop > 0 and target > entry:
        move = target / entry - 1.0
        fee_cost = (entry + target) * fee
        net_reward = (target - entry) - fee_cost
        risk = entry - stop
        net_rr = (net_reward / risk) if risk > 0 else 0.0
        checks.update({"target_move": round(move, 4), "net_rr": round(net_rr, 2)})
        if move < c["min_target_move"]:
            return {"ok": False, "checks": checks,
                    "reason": f"target only {move*100:.1f}% away (< {c['min_target_move']*100:.1f}%) — "
                              f"barely clears {fee*2*100:.1f}% round-trip fee"}
        if net_rr < c["min_net_rr"]:
            return {"ok": False, "checks": checks,
                    "reason": f"net RR {net_rr:.2f} after fees < {c['min_net_rr']:.1f} — "
                              f"edge too thin once costs are paid"}

    return {"ok": True, "checks": checks, "reason": "market quality ok"}
