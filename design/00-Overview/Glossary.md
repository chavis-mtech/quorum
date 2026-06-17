# Glossary

The shared vocabulary. When a term is capitalised elsewhere in the vault, it means the thing defined here.

| Term | Definition |
|------|------------|
| **Agent / Analyst** | One independent opinion-former in the council: `technical`, `trend_ml`, `finbert`, `cryptobert`, `news`. Each emits an action + confidence + reasoning. |
| **Council** | The full set of agents that run in parallel on a symbol. |
| **Consensus / Aggregator** | The weighted vote that combines agent opinions into a *preliminary* verdict. Requires **min agreement** (≥N agents agree) and a **confidence floor**, or it resolves to HOLD. |
| **Veto** | The news layer's power to force HOLD on a critical event (exploit, delisting), regardless of the vote. |
| **Judge** | The final decision-maker. An LLM (regime-aware) — or a deterministic **rule-based planner** if no LLM is available — that turns an actionable consensus into a concrete plan. |
| **Regime** | The market's character: `trending` (ADX>25, ER≥0.35), `weak-trend`, or `ranging` (ER<0.20). Drives entry style, RR thresholds, and sizing. See [[Entry-Strategy]]. |
| **Verdict** | The judge's structured output: action, confidence, entry_type, entry/target/stop prices, thesis, invalidation, size. |
| **Trade Plan** | A persisted intention to trade a symbol, with a lifecycle (Pending → Open → Closed/Cancelled). See [[Domain-Model]]. |
| **Entry type** | `market` (enter now, ride strength) vs `limit` (wait for a pullback). Chosen per regime by the [[Entry-Strategy]]. |
| **Entry discipline** | The deterministic anti-chase guard: converts an over-extended market BUY into a pullback limit — but only when the move is extreme *for its regime*. |
| **Preflight** | Pure pre-submission checks (cash sufficiency, min order, price>0) that reject doomed orders before any API call. |
| **Governor** | The capital/risk controller that answers "how many more buys are allowed, and why isn't it buying?" States: `scanning`, `full`, `halted`, `paused`, `signal`, `manual`. |
| **Kill-switch** | The user's manual `paused` flag — stops all new entries instantly. |
| **R / R-multiple** | Risk unit = entry − initial stop. Trailing logic is expressed in multiples of R. See [[Position-Management]]. |
| **Account kind** | A persistent property of an account: `paper` (simulated wallet) or `live` (real Bitkub balance). |
| **Trading mode** | A per-account *setting*: `paper`, `live`, or `signal-only` (analyse, never order). **Distinct from account kind** — a live account can run in signal-only mode. |
| **Exchange coin vs Broker coin** | Bitkub `source` field. `exchange` coins trade on Bitkub's own order book (API-tradable). `broker` coins are routed to a third party and the order API rejects them (**error 61**). Quorum filters broker coins out of the universe. See [[Broker-Integration]]. |
| **Discovery / Scanner** | The momentum scanner that auto-builds the watch universe from Bitkub tickers (|24h move| × liquidity). |
| **Tenant** | A user with isolated data, settings, and credentials, scoped by `account_id`. |
