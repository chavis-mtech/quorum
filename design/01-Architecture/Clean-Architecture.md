# Clean Architecture (the Rust core)

The Trading Core follows a strict **ports-and-adapters / clean architecture** layering. The point: business rules (risk, planning, P&L) never depend on a database driver or an HTTP client, so they stay pure and testable, and infrastructure can be swapped without touching the rules.

```mermaid
flowchart TB
    subgraph P [presentation]
        H[handlers · ws · middleware]
    end
    subgraph A [application]
        T[TradingService · Watcher]
        R[risk · governor · preflight]
    end
    subgraph D [domain]
        M[models]
        PO[ports / traits]
        ER[DomainError]
    end
    subgraph I [infrastructure]
        BK[bitkub / binance]
        PGI[postgres repos]
        AIS[ai_sidecar]
        SC[scanner · market]
        AU[auth]
    end

    H --> T
    T --> R
    T --> PO
    R --> M
    PO --> M
    BK -. implements .-> PO
    PGI -. implements .-> PO
    AIS -. implements .-> PO
    SC -. implements .-> PO

    classDef dom fill:#1e3a5f,stroke:#4a90d9,color:#fff;
    classDef app fill:#1e4f3a,stroke:#4ad98f,color:#fff;
    classDef inf fill:#3d2f1e,stroke:#d9a04a,color:#fff;
    classDef pre fill:#3a2d4f,stroke:#9d7ad9,color:#fff;
    class M,PO,ER dom; class T,R app; class BK,PGI,AIS,SC,AU inf; class H pre;
```

## The dependency rule

**Source code dependencies point only inward.** `domain` knows nothing about anyone. `application` depends on `domain`. `infrastructure` and `presentation` depend on `application` + `domain`. Crucially, infrastructure adapters **implement domain ports** (traits) — so the application calls an interface, and the concrete Bitkub/Postgres/LLM type is injected at startup.

## Layers

| Layer | Path | Contains | Depends on |
|-------|------|----------|------------|
| **domain** | `backend/src/domain/` | `models` (entities, enums, value objects), `ports` (traits: `Broker`, `TradeRepository`, `PlanRepository`, `AiEngine`, `MarketData`…), `DomainError` | nothing |
| **application** | `backend/src/application/` | `TradingService` (the orchestrator), `Watcher` (the loop), and the **pure rule modules**: `risk`, `governor`, `preflight` | domain |
| **infrastructure** | `backend/src/infrastructure/` | adapters: `bitkub`, `binance`, `postgres*`, `ai_sidecar`, `scanner`, `market`, `auth`, `qpack`, `broker_resolver` | application + domain |
| **presentation** | `backend/src/presentation/` | axum `handlers`, `ws`, `middleware`, shared `state` | application + domain |

## Why this pays off here

- **The money path is unit-testable without a broker or a DB.** `risk::evaluate`, `governor::evaluate`, `preflight::check_buy`, the trailing-stop maths, and realized-P&L walking are all **pure functions** with dozens of tests — no network, no Postgres. (66 cargo tests run in <1s.)
- **Swapping a broker is an adapter, not a rewrite.** `Broker` is a trait; `BitkubBroker` and a `Binance` skeleton both implement it. Per-tenant resolution happens behind `BrokerResolver`.
- **The LLM is just a port.** `AiEngine` hides whether reasoning came from Ollama, a cloud model, or the rule-based planner.

## Key seams (ports)

| Port (trait) | Adapter(s) | Hides |
|--------------|-----------|-------|
| `Broker` | `BitkubBroker`, `Binance`(skeleton), paper | order placement, balances, prices |
| `MarketData` / `MarketScanner` | `BitkubMarket`, `MomentumScanner` | the tradable universe & tickers |
| `TradeRepository` / `PlanRepository` / `SettingsStore` / `AlertStore` | `postgres*` | persistence + tenant scoping |
| `AiEngine` | `ai_sidecar` | the council/judge HTTP contract |
| `SecretStore` / `BrokerResolver` | `postgres` + factory | per-tenant credential resolution |

Related: [[Container-Architecture]] · [[Domain-Model]] · [[Order-Execution]]
