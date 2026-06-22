"""Edge — turn settled outcomes into a "should we take this setup?" decision.

Each decision is filed into a coarse SETUP BUCKET (a signature of the market state at decision
time). Buckets are kept coarse on purpose so they fill with samples quickly:

    bucket = "<regime>|<trend_dir>|<rsi_zone>"
      regime    : trending | weak-trend | ranging | unknown   (from trend_ml)
      trend_dir : up | down | sideways                          (from the trend gate)
      rsi_zone  : os (<35) | low (35-50) | mid (50-65) | hot (>=65)

For a NEW buy in bucket B we look at B's learned record:
  • block  — enough samples AND the bucket is a proven money-loser  → don't trade live; shadow it.
  • allow  — positive / unproven edge                               → trade live.
  • a gentle confidence multiplier nudges borderline buckets across the backend's min_confidence
    line (so weak-but-not-terrible setups naturally trade less).

A portfolio-level circuit breaker: when the last N LIVE trades are collectively bleeding, we stop
exploring unproven buckets (only proven-positive buckets may trade) until the bleeding stops.
"""
from __future__ import annotations

from typing import Any


# ── tuning (overridable via the [learning] config block) ─────────────────────────
DEFAULTS = {
    "min_samples_gate": 6,      # need this many settled trades before a bucket can BLOCK (faster to cut losers)
    "min_samples_conf": 5,      # ...and this many before nudging confidence
    "block_expectancy": -0.08,  # avg R at/below which a bucket is a proven loser → block
    "block_winrate_n": 8,       # with this many samples...
    "block_winrate": 0.34,      # ...a win-rate below this also blocks
    "breaker_min_samples": 10,  # circuit breaker needs this many recent live trades
    "breaker_expectancy": -0.15,# recent live avg R at/below this → defensive mode
    "conf_span": 0.5,           # confidence multiplier = clamp(1 + exp*span, lo, hi)
    "conf_lo": 0.75,
    "conf_hi": 1.25,            # let a strongly-proven winner clear the backend confidence floor
}


def rsi_zone(rsi: float) -> str:
    if rsi <= 0:
        return "na"
    if rsi < 35:
        return "os"
    if rsi < 50:
        return "low"
    if rsi < 65:
        return "mid"
    return "hot"


def bucket_key(regime: str, trend_dir: str, rsi: float) -> str:
    regime = (regime or "unknown").lower()
    trend_dir = (trend_dir or "sideways").lower()
    return f"{regime}|{trend_dir}|{rsi_zone(rsi)}"


def _stat(stats: dict[str, Any], bucket: str) -> dict[str, float]:
    s = stats.get(bucket) or {}
    n = int(s.get("n", 0))
    wins = int(s.get("wins", 0))
    sum_r = float(s.get("sum_r", 0.0))
    return {
        "n": n,
        "wins": wins,
        "win_rate": (wins / n) if n else 0.0,
        "expectancy": (sum_r / n) if n else 0.0,
    }


def update_stats(stats: dict[str, Any], bucket: str, r: float) -> None:
    """Fold one settled outcome into the persistent bucket aggregate (in place)."""
    s = stats.setdefault(bucket, {"n": 0, "wins": 0, "sum_r": 0.0})
    s["n"] = int(s.get("n", 0)) + 1
    s["sum_r"] = float(s.get("sum_r", 0.0)) + float(r)
    if r > 0:
        s["wins"] = int(s.get("wins", 0)) + 1


def recent_live_expectancy(recent_live: list[float]) -> tuple[int, float]:
    n = len(recent_live)
    return n, (sum(recent_live) / n if n else 0.0)


def decide(
    stats: dict[str, Any],
    bucket: str,
    recent_live: list[float],
    cfg: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Return the gate decision for a new long in `bucket`.

    {"action": "block"|"allow", "conf_mult": float, "reason": str, "edge": {...}, "defensive": bool}
    """
    c = {**DEFAULTS, **(cfg or {})}
    st = _stat(stats, bucket)
    n, wr, exp = st["n"], st["win_rate"], st["expectancy"]

    rl_n, rl_exp = recent_live_expectancy(recent_live)
    defensive = rl_n >= c["breaker_min_samples"] and rl_exp <= c["breaker_expectancy"]

    # proven money-loser bucket → block
    if n >= c["min_samples_gate"] and (
        exp <= c["block_expectancy"]
        or (n >= c["block_winrate_n"] and wr < c["block_winrate"])
    ):
        return {
            "action": "block", "conf_mult": 1.0, "defensive": defensive, "edge": st,
            "reason": (f"learned loser: {wr*100:.0f}% win / {exp:+.2f}R over {n} trades "
                       f"in setup [{bucket}] → BUY blocked, shadow-tracking instead"),
        }

    # circuit breaker: bleeding live → only proven-positive buckets may trade
    if defensive and not (n >= c["min_samples_gate"] and exp > 0.05):
        return {
            "action": "block", "conf_mult": 1.0, "defensive": True, "edge": st,
            "reason": (f"defensive mode: last {rl_n} live trades {rl_exp:+.2f}R — pausing unproven "
                       f"setup [{bucket}] (n={n}, exp={exp:+.2f}R) until results recover"),
        }

    # confidence nudge for buckets with some history
    conf_mult = 1.0
    if n >= c["min_samples_conf"]:
        conf_mult = max(c["conf_lo"], min(c["conf_hi"], 1.0 + exp * c["conf_span"]))

    if n == 0:
        reason = f"no history yet for setup [{bucket}] — exploring (live, recording outcome)"
    else:
        verdict = "edge" if exp > 0 else "thin"
        reason = (f"learned {verdict}: {wr*100:.0f}% win / {exp:+.2f}R over {n} trades "
                  f"in setup [{bucket}] → allowed (conf×{conf_mult:.2f})")
    return {"action": "allow", "conf_mult": round(conf_mult, 3),
            "defensive": defensive, "edge": st, "reason": reason}
