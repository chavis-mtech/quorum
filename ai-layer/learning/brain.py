"""Brain — orchestrates the self-learning loop for one analysis cycle.

Called once per /analyze, after the judge has produced a verdict. It:

  1) SETTLES open decisions for this symbol against the freshly-fetched candles (did past calls
     hit target or stop?) and folds the realized R into the learned per-bucket stats.
  2) GATES the new verdict: a BUY in a proven-loser setup, or in chop / fee-unviable conditions,
     is blocked (→ HOLD) and recorded as a SHADOW trade instead of a live one.
  3) RECORDS this cycle's decision (live or shadow) so a future cycle can settle it.

All of it is best-effort: any exception is swallowed by the caller so a learning failure can
never block trading. Everything persists in <runtime_root>/var/brain (survives ai-layer swaps).
"""
from __future__ import annotations

from typing import Any

from strategy.trend_gate import trend_direction
from strategy import market_quality

from . import edge, journal, settle


def _levels_for(entry: float, target: float, stop: float,
                structure: dict[str, Any], min_move: float) -> tuple[float, float, float]:
    """Use the verdict's own entry/target/stop when valid; otherwise synthesise a realistic
    long (ATR-based, target guaranteed to clear fees) so a shadow trade is still measurable."""
    e = entry if entry > 0 else float(structure.get("price") or 0.0)
    if e <= 0:
        return 0.0, 0.0, 0.0
    if target > e > stop > 0:
        return e, target, stop
    atr_pct = float(structure.get("atr_pct") or 0.0) or 0.01
    s = e * (1.0 - max(0.01, 1.5 * atr_pct))
    t = e * (1.0 + max(min_move, 2.5 * atr_pct))
    return e, t, s


def _overall(stats: dict[str, Any]) -> dict[str, Any]:
    n = sum(int(v.get("n", 0)) for v in stats.values())
    wins = sum(int(v.get("wins", 0)) for v in stats.values())
    sum_r = sum(float(v.get("sum_r", 0.0)) for v in stats.values())
    return {"n": n, "win_rate": (wins / n) if n else 0.0,
            "expectancy": (sum_r / n) if n else 0.0}


def evaluate(symbol: str, candles: list[dict[str, float]], structure: dict[str, Any],
             consensus: dict[str, Any], verdict: dict[str, Any], regime: str,
             synthetic: bool, cfg: dict[str, Any]) -> dict[str, Any]:
    """Settle past → learn → decide gate → record. Returns a learning-info dict (does NOT mutate
    the verdict; call apply() for that)."""
    if synthetic or len(candles) < 10:
        return {"enabled": False, "summary": "🧠 learning skipped (offline/synthetic data)"}

    lcfg = dict(cfg.get("learning") or {})
    mqcfg = dict(cfg.get("market_quality") or {})
    fee = float(lcfg.get("fee_per_side", settle.DEFAULT_FEE_PER_SIDE))
    max_bars = int(lcfg.get("max_settle_bars", 48))
    min_move = float(mqcfg.get("min_target_move", market_quality.DEFAULTS["min_target_move"]))
    # re-entry cooldown: after a real loss on a coin, don't immediately re-buy it (anti revenge-trade
    # / falling-knife re-entry — a direct fix for the negative-skew losses we measured).
    cooldown_bars = int(lcfg.get("reentry_cooldown_bars", 3))
    loss_thresh = float(lcfg.get("reentry_loss_threshold", -0.5))

    data = journal.load()
    open_entries = data["open"]
    stats = data["stats"]
    recent_live = data["recent_live"]
    cooldowns = dict(data.get("cooldowns") or {})

    bar_ts = int(candles[-1].get("ts") or 0)
    oldest_ts = int(candles[0].get("ts") or 0)
    # candle spacing (seconds) → converts the cooldown window into wall-clock
    step = 3600
    if len(candles) >= 2:
        _s = int(candles[-1].get("ts") or 0) - int(candles[-2].get("ts") or 0)
        if _s > 0:
            step = _s

    # ── 1) settle open decisions for this symbol ─────────────────────────────────
    settled_notes: list[dict[str, Any]] = []
    still_open: list[dict[str, Any]] = []
    for e in open_entries:
        if e.get("symbol") != symbol:
            still_open.append(e)
            continue
        ts0 = int(e.get("ts") or 0)
        if ts0 < oldest_ts:
            continue  # entry predates our candle window → can't see its path, drop uncounted
        bars_after = [c for c in candles if int(c.get("ts") or 0) > ts0]
        if not bars_after:
            still_open.append(e)
            continue
        res = settle.settle_planned_long(
            str(e.get("entry_type") or "market"),
            float(e.get("entry", 0)),
            float(e.get("target", 0)),
            float(e.get("stop", 0)),
            bars_after,
            max_entry_bars=int(lcfg.get("max_entry_bars", 12)),
            max_bars=max_bars,
            fee_per_side=fee,
        )
        if res["status"] in ("win", "loss", "expired"):
            edge.update_stats(stats, e.get("bucket", "unknown"), res["r"])
            if e.get("kind") == "live":
                recent_live.append(res["r"])
            if res["r"] is not None and res["r"] <= loss_thresh:
                cooldowns[symbol] = bar_ts  # just lost on this coin → cool off before re-entering
            settled_notes.append({"kind": e.get("kind"), "bucket": e.get("bucket"),
                                  "status": res["status"], "r": res["r"]})
        elif res["status"] in ("open", "pending"):
            still_open.append(e)
        elif res["status"] == "missed":
            settled_notes.append({"kind": e.get("kind"), "bucket": e.get("bucket"),
                                  "status": "missed", "r": None})
        # "invalid" → drop silently

    # ── 2) classify the current setup + gate the verdict ─────────────────────────
    rsi = float(structure.get("rsi") or 0.0)
    tdir = trend_direction(structure)
    bucket = edge.bucket_key(regime, tdir, rsi)
    price = float(structure.get("price") or candles[-1].get("close") or 0.0)

    action = str(verdict.get("action") or "HOLD").upper()
    v_entry = float(verdict.get("entry_price") or 0.0)
    entry_px = v_entry if v_entry > 0 else price
    target = float(verdict.get("target_price") or 0.0)
    stop = float(verdict.get("stop_price") or 0.0)

    block = False
    block_kind: str | None = None
    reason = ""
    conf_mult = 1.0
    if action == "BUY":
        cd_ts = int(cooldowns.get(symbol, 0) or 0)
        on_cooldown = cd_ts and 0 <= (bar_ts - cd_ts) < cooldown_bars * step
        if on_cooldown:
            bars_left = max(1, cooldown_bars - (bar_ts - cd_ts) // step)
            block, block_kind = True, "cooldown"
            reason = (f"re-entry cooldown: a recent {symbol} trade settled at a loss "
                      f"(~{bars_left} bar(s) left) — not chasing it straight back")
        else:
            quality = market_quality.assess_quality(structure, regime, entry_px, target, stop, mqcfg)
            if not quality["ok"]:
                block, block_kind, reason = True, "quality", quality["reason"]
            else:
                gate = edge.decide(stats, bucket, recent_live, lcfg)
                conf_mult = float(gate.get("conf_mult", 1.0))
                reason = gate.get("reason", "")
                if gate.get("action") == "block":
                    block, block_kind = True, "learned"

    # ── 3) record this cycle's decision (dedup one per bar per symbol) ───────────
    already = any(int(e.get("ts") or 0) == bar_ts and e.get("symbol") == symbol
                  for e in still_open)
    recorded_kind: str | None = None
    if not already:
        lean_long = (action == "BUY"
                     or str(consensus.get("action")) == "BUY"
                     or float(structure.get("prob_up") or 0.5) > 0.55)
        if action == "BUY" and not block:
            recorded_kind = "live"
        elif lean_long:
            recorded_kind = "shadow"
        if recorded_kind:
            re_, rt_, rs_ = _levels_for(entry_px, target, stop, structure, min_move)
            if re_ > 0 and rt_ > re_ > rs_ > 0:
                still_open.append({
                    "symbol": symbol, "ts": bar_ts, "kind": recorded_kind,
                    "entry_type": str(verdict.get("entry_type") or "market").lower(),
                    "entry": round(re_, 8), "target": round(rt_, 8), "stop": round(rs_, 8),
                    "bucket": bucket, "regime": regime, "rsi": round(rsi, 1),
                    "conviction": verdict.get("conviction"),
                    "reversal_risk": verdict.get("reversal_risk"),
                })
            else:
                recorded_kind = None

    # keep only live (non-stale, non-future) cooldowns so the map stays small
    cooldowns = {k: v for k, v in cooldowns.items()
                 if 0 <= (bar_ts - int(v or 0)) < 7 * 86400}
    data["open"] = still_open
    data["stats"] = stats
    data["recent_live"] = recent_live
    data["cooldowns"] = cooldowns
    journal.save(data)

    # ── scoreboards + human summary ──────────────────────────────────────────────
    bstat = edge._stat(stats, bucket)
    overall = _overall(stats)
    rl_n, rl_exp = edge.recent_live_expectancy(recent_live)

    if block:
        head = f"🧠 BUY held back ({block_kind}) — {reason}"
    elif action == "BUY":
        head = f"🧠 BUY allowed — {reason}"
    elif recorded_kind == "shadow":
        head = (f"🧠 shadow-tracking a hypothetical long in [{bucket}] "
                f"(no live trade this cycle) to keep learning")
    else:
        head = f"🧠 learning: setup [{bucket}], no directional record this cycle"

    scoreboard = (f"learned {overall['n']} settled "
                  f"(win {overall['win_rate']*100:.0f}% · {overall['expectancy']:+.2f}R) · "
                  f"live last {rl_n} @ {rl_exp:+.2f}R · "
                  f"this setup n={bstat['n']} {bstat['expectancy']:+.2f}R")
    if settled_notes:
        wins = sum(1 for s in settled_notes if s["status"] == "win")
        scoreboard += f" · settled now: {len(settled_notes)} ({wins}W)"

    return {
        "enabled": True,
        "bucket": bucket,
        "trend_dir": tdir,
        "block": block,
        "block_kind": block_kind,
        "reason": reason,
        "conf_mult": round(conf_mult, 3),
        "recorded": recorded_kind,
        "edge": {"n": bstat["n"], "win_rate": round(bstat["win_rate"], 3),
                 "expectancy": round(bstat["expectancy"], 3)},
        "overall": {"n": overall["n"], "win_rate": round(overall["win_rate"], 3),
                    "expectancy": round(overall["expectancy"], 3)},
        "live_scoreboard": {"n": rl_n, "expectancy": round(rl_exp, 3)},
        "settled_now": settled_notes,
        "summary": head + " · " + scoreboard,
    }


def apply(verdict: dict[str, Any], info: dict[str, Any]) -> dict[str, Any]:
    """Mutate the verdict per the learning decision: block a BUY (→ HOLD) or scale confidence."""
    if not info or not info.get("enabled"):
        return verdict
    action = str(verdict.get("action") or "HOLD").upper()
    if info.get("block") and action == "BUY":
        verdict["action"] = "HOLD"
        verdict["entry_type"] = "none"
        verdict["entry_price"] = 0.0
        verdict["suggested_size_pct"] = 0.0
        if not verdict.get("invalidation"):
            verdict["invalidation"] = "wait for a setup the learner rates positively"
        verdict["reasoning"] = (str(verdict.get("reasoning", "")) +
                                f" | 🧠 learning-gate: {info.get('reason', '')}")[:4000]
    else:
        cm = float(info.get("conf_mult") or 1.0)
        if abs(cm - 1.0) > 1e-6:
            cur = float(verdict.get("confidence") or 0.0)
            verdict["confidence"] = round(max(0.0, min(1.0, cur * cm)), 4)
            if action == "BUY":
                verdict["reasoning"] = (str(verdict.get("reasoning", "")) +
                                        f" | 🧠 learning: {info.get('reason', '')}")[:4000]
    return verdict
