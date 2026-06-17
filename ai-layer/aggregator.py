"""Aggregator — combines votes from agents into a single consensus decision (the heart of consensus)

Rules:
  1) veto: if any agent has veto=True → force HOLD immediately
  2) structural requirement: BUY/SELL must have technical or trend_ml in agreement
     (sentiment alone is not allowed to win)
  3) weighted vote: weight = config_weight × confidence of each agent
  4) consensus threshold:
       - number of agents (ok=True) agreeing with the winning side ≥ min_agreement
       - weighted-average confidence ≥ min_confidence
       - BUY requires slightly higher confidence than SELL (asymmetric: entering is harder than exiting)
     if not met → HOLD
  5) expose regime from trend_ml so judge can adjust threshold accordingly
"""
from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Any

from agents.base import AgentResult, BUY, SELL, HOLD

# Structural agents = use real price action, at least 1 must agree for signal to pass
_STRUCTURAL = {"technical", "trend_ml"}


@dataclass
class ConsensusResult:
    action: str
    confidence: float
    agreement: int
    voted: int
    vetoed: bool
    reasoning: str
    tally: dict[str, float]
    votes: list[dict[str, Any]] = field(default_factory=list)
    passed_threshold: bool = True
    regime: str = "unknown"              # from trend_ml extra.regime

    def to_dict(self) -> dict[str, Any]:
        return {
            "action": self.action, "confidence": round(self.confidence, 4),
            "agreement": self.agreement, "voted": self.voted,
            "vetoed": self.vetoed, "passed_threshold": self.passed_threshold,
            "reasoning": self.reasoning, "tally": self.tally, "votes": self.votes,
            "regime": self.regime,
        }


def _extract_regime(results: list[AgentResult]) -> str:
    """Extract regime from trend_ml extra if available"""
    for r in results:
        if r.agent == "trend_ml" and r.ok and r.extra:
            return str(r.extra.get("regime", "unknown"))
    return "unknown"


def aggregate(results: list[AgentResult], *, weights: dict[str, float] | None = None,
              min_agreement: int = 3, min_confidence: float = 0.60) -> ConsensusResult:
    weights = weights or {}
    votes = [r.to_dict() for r in results]
    regime = _extract_regime(results)

    # 1) veto
    vetoer = next((r for r in results if r.can_veto and r.veto), None)
    if vetoer is not None:
        return ConsensusResult(
            action=HOLD, confidence=vetoer.confidence, agreement=0,
            voted=sum(1 for r in results if r.ok), vetoed=True,
            reasoning=f"VETO by {vetoer.agent}: {vetoer.reasoning}",
            tally={}, votes=votes, passed_threshold=False, regime=regime,
        )

    active = [r for r in results if r.ok]
    if not active:
        return ConsensusResult(HOLD, 0.0, 0, 0, False,
                               "No agents available to vote", {}, votes, False, regime)

    # 2) weighted tally
    tally: dict[str, float] = {BUY: 0.0, SELL: 0.0, HOLD: 0.0}
    conf_sum: dict[str, float] = {BUY: 0.0, SELL: 0.0, HOLD: 0.0}
    count: dict[str, int] = {BUY: 0, SELL: 0, HOLD: 0}
    for r in active:
        w = weights.get(r.agent, 1.0) * r.confidence
        tally[r.action] += w
        conf_sum[r.action] += r.confidence
        count[r.action] += 1

    winner = BUY if tally[BUY] >= tally[SELL] else SELL
    if tally[winner] <= 0:
        winner = HOLD

    agreement = count.get(winner, 0)
    avg_conf = (conf_sum[winner] / agreement) if agreement else 0.0

    # 3) threshold — adjusted to actual pool size (60% majority)
    effective_min = min(min_agreement, math.ceil(len(active) * 3 / 5)) if active else min_agreement

    # Asymmetric threshold: BUY requires slightly higher confidence than SELL
    # (entering a position is harder than exiting — prevents overtrading)
    conf_required = min_confidence + 0.03 if winner == BUY else min_confidence - 0.02

    # 4) Structural requirement: BUY/SELL must have technical or trend_ml in agreement
    structural_agrees = any(
        r.agent in _STRUCTURAL and r.action == winner
        for r in active
    )
    structural_warn = ""
    if winner in (BUY, SELL) and not structural_agrees:
        # No structural agent in agreement → reduce confidence by 20% before checking threshold
        avg_conf *= 0.80
        structural_warn = (f" WARNING: no technical/trend_ml agreeing with {winner} "
                           f"→ confidence reduced by 20% (now {avg_conf:.2f})")

    passed = (winner in (BUY, SELL)
              and agreement >= effective_min
              and avg_conf >= conf_required)

    tally_txt = (f"score BUY={tally[BUY]:.2f} · SELL={tally[SELL]:.2f} · HOLD={tally[HOLD]:.2f}")
    if not passed:
        miss = []
        if winner not in (BUY, SELL):
            miss.append("no BUY/SELL side with positive weight")
        else:
            if agreement < effective_min:
                miss.append(f"agreement for {winner}: {agreement}/{len(active)} (need ≥{effective_min})")
            if avg_conf < conf_required:
                miss.append(f"conf {avg_conf:.2f} below threshold {conf_required:.2f} (asymmetric {'BUY' if winner==BUY else 'SELL'})")
        reason = (f"threshold not met → HOLD. {tally_txt}.{structural_warn} "
                  f"missing: {' and '.join(miss) or '-'}. regime={regime}")
        return ConsensusResult(HOLD, avg_conf, agreement, len(active), False,
                               reason, tally, votes, False, regime)

    reason = (f"consensus {winner}: agreement {agreement}/{len(active)} agents, "
              f"avg conf {avg_conf:.2f} (passed threshold ≥{effective_min}/{conf_required:.2f}). "
              f"{tally_txt}.{structural_warn} regime={regime}")
    return ConsensusResult(winner, avg_conf, agreement, len(active), False,
                           reason, tally, votes, True, regime)
