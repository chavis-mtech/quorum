# Data Model (PostgreSQL ERD)

Every business table is **scoped by `account_id`**. Multi-tenancy is not a feature bolted on — it's the spine of the schema (migration `0006_identity` replaced the original singletons with per-account tables).

```mermaid
erDiagram
    USERS ||--o{ ACCOUNTS : owns
    ACCOUNTS ||--|| ACCOUNT_SETTINGS : configures
    ACCOUNTS ||--|| ACCOUNT_WALLET : "paper funds"
    ACCOUNTS ||--o{ TRADE_PLANS : has
    ACCOUNTS ||--o{ TRADES : has
    ACCOUNTS ||--o{ DECISIONS : has
    ACCOUNTS ||--o{ POSITIONS : holds
    ACCOUNTS ||--o{ WATCHLIST : tracks
    ACCOUNTS ||--o{ ALERTS : raises
    DECISIONS ||--o{ TRADES : "linked to"
    DECISIONS ||--o| TRADE_PLANS : produces

    USERS {
        bigint id PK
        text email UK
        text password_hash "argon2"
        timestamptz created_at
    }
    ACCOUNTS {
        bigint id PK
        bigint user_id FK
        text kind "paper | live"
    }
    ACCOUNT_SETTINGS {
        bigint account_id FK
        text mode "paper|live|signal-only"
        bool auto_trade
        bool paused "kill-switch"
        double trade_amount_quote
        int max_open_positions
        double daily_loss_limit
        double min_confidence
        text manage_style
        bool let_winners_run
        text broker "bitkub | binance"
    }
    TRADE_PLANS {
        bigint id PK
        bigint account_id FK
        text symbol
        text state "Pending|Open|Closed|Cancelled"
        text entry_type
        double entry_price
        double target_price
        double stop_price
        double high_water_mark
        double initial_stop
        bool trail_active
        timestamptz created_at
    }
    TRADES {
        bigint id PK
        bigint account_id FK
        bigint decision_id FK
        text symbol
        text side
        text mode
        double amount_base
        double amount_quote
        double price
        text status "filled|failed"
        double realized_pnl
        text note "broker reason on failure"
    }
    DECISIONS {
        bigint id PK
        bigint account_id FK
        text symbol
        text action
        double confidence
        jsonb analysis_json "full trace"
    }
    POSITIONS {
        bigint account_id FK
        text symbol
        double amount_base
        double avg_price
    }
    ALERTS {
        bigint id PK
        bigint account_id FK
        text level
        text code
        text message
    }
```

## Tenancy & access pattern

- A request arrives with a **JWT** (identifies the user) and an **`X-Account-Id`** header. Middleware validates the account belongs to the user and injects `Ctx{user_id, account_id, account_kind}`.
- Every repository method takes `account_id` and filters on it — there is no "global" query path.
- **WebSocket** events carry an `account_id` and are filtered per subscriber, so one tenant never receives another's trade/alert stream.

## Notable design choices

| Choice | Rationale |
|--------|-----------|
| `trades.note` stores the broker's failure reason | So a `failed` order is explainable (e.g. *"Bitkub error 61: broker coin"*) — surfaced in the UI and Alerts. See [[Broker-Integration]]. |
| `decisions.analysis_json` is `jsonb` | The full reasoning trace is replayable from the UI ("view reasoning"). |
| Plans carry `initial_stop` + `high_water_mark` + `trail_active` | Enables R-multiple trailing without recomputing history. See [[Position-Management]]. |
| Realized P&L is recomputed from the trade ledger | Broker-independent correctness; survives a broker reporting `avg=0`. |
| Migrations are append-only and checksum-locked (sqlx) | An applied migration is never edited — new behaviour = new migration. |

Related: [[Domain-Model]] · [[Deployment-and-Security]]
