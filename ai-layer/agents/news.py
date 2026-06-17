"""News verification agent — strictly verifies news (final safety layer)

Responsibilities:
  1) Evaluate overall sentiment from headlines — clearly state which "keyword" was found in which "headline"
  2) If a "critical" news event is found (hack, ban, fraud, delist, exploit, etc.) → VETO immediately
  3) If a "warning" news event is found (regulatory concern, warning, etc.) → no veto but increase bearish weight

This agent is the only one with can_veto=True
"""
from __future__ import annotations

from .base import Agent, AgentResult, MarketContext, BUY, SELL, HOLD

# Keywords that trigger immediate VETO — events that may cause a coin to collapse or be suspended
_CRITICAL = {
    # Security / exploit
    "hack", "hacked", "exploit", "exploited", "stolen", "theft", "breach",
    "vulnerability", "zero-day", "attack", "compromised",
    # Legal / regulatory action
    "ban", "banned", "delist", "delisted", "halt", "halted",
    "sec sues", "sec charges", "cftc", "indicted", "arrested", "seized",
    "shutdown", "investigation", "charges", "convicted", "money laundering",
    # Fraud / insolvency
    "fraud", "scam", "rug pull", "exit scam", "ponzi", "insolvent",
    "bankrupt", "bankruptcy", "insolvency", "frozen", "liquidated",
    "default", "unable to pay", "withdrawal suspended",
}

# Keywords that are concerning but do not reach veto level (increase bearish weight)
_WARNING = {
    "warning", "risk", "concern", "caution", "scrutiny", "probe",
    "regulatory", "regulation", "crackdown", "oversight", "compliance",
    "bear market", "recession", "inflation", "rate hike", "tightening",
    "outflow", "sell pressure", "profit taking", "overbought",
}

_POS = {
    "surge", "rally", "adoption", "partnership", "upgrade", "approval",
    "etf", "record", "breakout", "bullish", "soar", "gain", "institutional",
    "listing", "listed", "launch", "integration", "milestone", "all-time high",
    "accumulation", "buyback", "burn", "staking", "ecosystem", "mainnet",
    "whale accumulation", "inflow", "spot etf", "halving",
}
_NEG = {
    "plunge", "crash", "selloff", "dump", "fear", "decline",
    "bearish", "drop", "loss", "downgrade", "outflow", "liquidation",
    "sell-off", "correction", "slump", "tumble", "risk-off", "panic",
    "flash crash", "massive selloff", "blood", "capitulation",
}


def _classify(headlines: list[str]) -> tuple[
    float, list[str], list[str], dict[str, list[str]], dict[str, list[str]]
]:
    """Return (sentiment_score, critical_hits, warning_hits, pos_hits, neg_hits)"""
    score = 0.0
    crit: list[str] = []
    warn: list[str] = []
    pos_hits: dict[str, list[str]] = {}
    neg_hits: dict[str, list[str]] = {}
    for h in headlines:
        low = h.lower()
        hit_crit = False
        for kw in _CRITICAL:
            if kw in low:
                crit.append(h)
                hit_crit = True
                break
        if not hit_crit:
            for kw in _WARNING:
                if kw in low:
                    warn.append(h)
                    score -= 0.3  # warning reduces sentiment but does not veto
                    break
        for w in _POS:
            if w in low:
                score += 0.5
                pos_hits.setdefault(w, []).append(h)
        for w in _NEG:
            if w in low:
                score -= 0.5
                neg_hits.setdefault(w, []).append(h)
    return score, crit, warn, pos_hits, neg_hits


def _fmt_hits(hits: dict[str, list[str]], limit: int = 3) -> str:
    parts = []
    for kw, hs in list(hits.items())[:limit]:
        sample = hs[0][:70] + ("…" if len(hs[0]) > 70 else "")
        parts.append(f'"{kw}"×{len(hs)} (e.g.: {sample})')
    return " · ".join(parts)


class NewsAgent(Agent):
    name = "news"
    can_veto = True

    def analyze(self, ctx: MarketContext) -> AgentResult:
        headlines: list[str] = ctx.extra.get("headlines", [])
        if not headlines:
            return AgentResult(
                agent=self.name, action=HOLD, confidence=0.2,
                reasoning="No news found to verify — no veto but neutral vote (low confidence)",
                can_veto=True, veto=False, ok=True,
            )

        score, crit, warn, pos_hits, neg_hits = _classify(headlines)

        # VETO: critical news found
        if crit:
            detail = f'"{crit[0][:100]}"' + (f" + {len(crit)-1} more" if len(crit) > 1 else "")
            return AgentResult(
                agent=self.name, action=HOLD, confidence=0.95,
                reasoning=(f"🚨 VETO: Found {len(crit)} critical news item(s) — blocking trade immediately\n"
                           f"News: {detail}"),
                can_veto=True, veto=True, ok=True,
                extra={"critical": crit, "sentiment": score},
            )

        lines = [f"Checked {len(headlines)} headlines — no CRITICAL event found"]
        if warn:
            lines.append(f"⚠️ Found {len(warn)} warning news item(s) (no veto but adds bearish pressure):")
            for w in warn[:2]:
                lines.append(f"  • {w[:80]}")
        if pos_hits:
            lines.append(f"🟢 Positive keywords: {_fmt_hits(pos_hits)}")
        if neg_hits:
            lines.append(f"🔴 Negative keywords: {_fmt_hits(neg_hits)}")
        if not pos_hits and not neg_hits and not warn:
            lines.append("News is neutral at this time — no directional signal")

        if score > 0.5:
            action, conf = BUY, min(0.82, 0.40 + score * 0.10)
            lines.append(f"🧮 sentiment {score:+.1f} → positive (conf {conf:.2f})")
        elif score < -0.5:
            action, conf = SELL, min(0.82, 0.40 + abs(score) * 0.10)
            lines.append(f"🧮 sentiment {score:+.1f} → negative (conf {conf:.2f})")
        else:
            action, conf = HOLD, 0.30
            lines.append(f"🧮 sentiment {score:+.1f} → neutral")

        return AgentResult(
            agent=self.name, action=action, confidence=conf,
            reasoning="\n".join(lines),
            can_veto=True, veto=False,
            extra={"sentiment": score, "warnings": len(warn)},
        )
