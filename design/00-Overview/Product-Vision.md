# Product Vision

## The problem

Retail crypto traders lose money for boring, repeatable reasons: they chase pumps, they over-trade in choppy markets, they let losers run and cut winners early, and they have no discipline that survives their own emotions. Off-the-shelf "trading bots" replace human emotion with a single brittle rule (one indicator, one model) that works until the regime changes.

## The bet

**A panel of specialists, forced to agree, makes better decisions than any single oracle — and a disciplined execution layer beats a smarter signal.** Quorum is built around that bet:

1. **Diverse analysts** — technical, ML trend, two sentiment models, and a news/veto layer — each look at the market independently.
2. **A weighted aggregator** requires genuine agreement (not a coin-flip majority) and a confidence floor before anything is actionable.
3. **A judge** (LLM, regime-aware) turns an actionable consensus into a concrete plan: entry style, target, stop, size — with iron risk rules.
4. **An execution core** that pre-validates every order, manages the open position like a professional (trailing stop, breakeven, thesis re-evaluation), and never risks more than the configured budget.

See [[Analysis-Pipeline]] and [[Entry-Strategy]] for how this is realised.

## Who it serves

- **The owner-operator** running their own capital on Bitkub today.
- **Multi-tenant from day one** — each user gets isolated paper + live accounts, their own broker credentials, their own risk settings. This is the seed of a SaaS product, not a single-user script. See [[Data-Model-ERD]] and [[Enterprise-Operating-Model]].

## Non-goals

- **High-frequency / latency arbitrage.** Quorum thinks on the minutes-to-hours timescale ("deep analysis" is infrequent by design — see [[Order-Execution]]).
- **A black box.** Every decision carries a human-readable thesis, an invalidation, and a full reasoning trace. Explainability is a feature, not an afterthought.
- **Maximising trade count.** The edge is selectivity. "Trade rarely, trade well."

## Principles (the constitution)

> These are the rules every design decision is checked against.

1. **Consensus over a single oracle.** No single agent — not even the LLM judge — can unilaterally force a trade.
2. **Capital preservation first.** Hard stops and a daily-loss governor are always on, independent of any model's opinion.
3. **Match the regime.** Entry/exit/sizing all adapt to trending vs ranging markets. ([[Entry-Strategy]])
4. **Determinism where it counts.** Realized P&L, risk caps, and preflight checks are pure, testable functions — not LLM guesses.
5. **Tenant isolation is sacred.** One user's keys, data, and orders never touch another's.
6. **Observable & reversible.** A kill-switch, a governor state, and an alert stream make "what is the bot doing and why?" answerable at any instant.

## Success metrics

| Metric | Why it matters |
|--------|----------------|
| Profit factor (gross win ÷ gross loss) | The honest measure — is the edge real? |
| Win rate × avg win/avg loss | Decomposes the edge into selectivity vs payoff |
| % of signals that become filled trades | Catches "all analysis, no action" pathologies |
| Max daily drawdown vs governor limit | Is risk control working? |
| Decision→fill latency | Are we missing setups we identified? |

Related: [[System-Context]] · [[Enterprise-Operating-Model]]
