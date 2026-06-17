# Domain Model

The core nouns and the one state machine that matters most.

## Core entities

```mermaid
classDiagram
    class Account {
        +id
        +user_id
        +kind: paper|live
    }
    class AccountSettings {
        +mode: paper|live|signal-only
        +auto_trade
        +trade_amount_quote
        +max_open_positions
        +daily_loss_limit
        +min_confidence
        +manage_style
        +broker: bitkub|binance
    }
    class TradePlan {
        +state: Pending|Open|Closed|Cancelled
        +action
        +entry_type: market|limit
        +entry/target/stop
        +high_water_mark
        +initial_stop
        +trail_active
    }
    class Decision {
        +verdict (action, conf)
        +consensus snapshot
        +analysis_json (trace)
    }
    class TradeRecord {
        +side
        +amount_base/quote
        +price
        +status: filled|failed
        +realized_pnl
        +note
    }
    class Position {
        +amount_base
        +avg_price
    }
    Account "1" --> "1" AccountSettings
    Account "1" --> "*" TradePlan
    Account "1" --> "*" Decision
    Account "1" --> "*" TradeRecord
    Decision "1" --> "0..1" TradePlan : produces
    TradePlan "1" --> "0..1" Position : when Open
    Decision "1" --> "*" TradeRecord : links
```

## Account kind vs. trading mode (a deliberate distinction)

These are **two different axes** and conflating them caused a real bug (a signal-only live account once showed a fake paper balance):

- **Account kind** (`paper` | `live`) — *what the money is.* Determines which wallet/balance is real. Persistent.
- **Trading mode** (`paper` | `live` | `signal-only`) — *what the bot is allowed to do right now.* A live account can run `signal-only` (analyse, never order). A setting, changeable anytime.

Balance display follows **kind**; the order/no-order decision follows **mode**. See [[Glossary]].

## The Trade-Plan lifecycle

The single most important state machine in the system.

```mermaid
stateDiagram-v2
    [*] --> Pending: limit entry chosen<br/>(wait for pullback)
    [*] --> Open: market entry filled<br/>(enter now)

    Pending --> Open: price reaches entry<br/>→ re-confirm → execute
    Pending --> Cancelled: price hit target first<br/>("missed the move")
    Pending --> Cancelled: price broke stop first<br/>(thesis invalid)
    Pending --> Pending: >12h old → re-analyse

    Open --> Open: each tick: trail stop /<br/>raise to breakeven /<br/>re-evaluate thesis
    Open --> Closed: target / trailing-stop /<br/>stop-loss / thesis broken

    Cancelled --> [*]
    Closed --> [*]
```

| Transition | Trigger | Owner |
|------------|---------|-------|
| → Pending | Judge chose a `limit` entry (regime says wait) | `track_pending` |
| → Open (immediate) | Judge chose `market` + confidence ≥ floor | `enter_now` → `execute` |
| Pending → Open | Live price reaches entry, re-confirmed by fresh analysis | `check_triggers` → `confirm_and_enter` |
| Pending → Cancelled | Target or stop hit *before* entry, or buy rejected | `check_triggers` |
| Open → Open | Trailing/breakeven update; thesis re-eval | `check_exits` + deep re-analysis |
| Open → Closed | Exit condition met | `check_exits` → `execute(SELL)` |

> The new [[Entry-Strategy]] shifts the **→ Open (immediate)** path to fire much more often in trending regimes — the previous design almost always took **→ Pending** and then **→ Cancelled (missed the move)**.

## Value objects & invariants

- **R (risk unit)** = `entry − initial_stop`. Trailing is expressed in R. ([[Position-Management]])
- **Realized P&L** is computed deterministically from the local trade ledger (`(fill − avg_cost) × qty`), never trusted from the broker.
- **Stops only ratchet up** — `refresh_review` and trailing use `GREATEST`; risk is never widened.
- **A hard catastrophic cap** (`MAX_LOSS_PCT ≈ 6%`) floors every exit stop regardless of the plan.

Related: [[Data-Model-ERD]] · [[Position-Management]]
