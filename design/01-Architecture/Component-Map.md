# Component Map (C4 — Level 3)

Inside the two core containers.

## Trading Core (Rust)

```mermaid
flowchart LR
    subgraph pres [presentation]
        api[handlers<br/>REST]
        ws[ws hub<br/>per-account fan-out]
        mw[auth middleware]
    end
    subgraph app [application]
        svc[TradingService]
        watch[Watcher loop]
        risk[risk]
        gov[governor]
        pre[preflight]
    end
    subgraph infra [infrastructure]
        bres[BrokerResolver]
        bk[BitkubBroker]
        mkt[BitkubMarket]
        scan[MomentumScanner]
        ais[AiSidecar client]
        repo[Postgres repos]
        auth[auth/JWT/argon2]
    end

    api --> svc
    ws --> svc
    mw --> auth
    watch --> svc
    svc --> risk & gov & pre
    svc --> bres --> bk
    svc --> mkt & scan & ais & repo

    classDef app fill:#1e4f3a,stroke:#4ad98f,color:#fff;
    classDef infra fill:#3d2f1e,stroke:#d9a04a,color:#fff;
    classDef pres fill:#3a2d4f,stroke:#9d7ad9,color:#fff;
    class svc,watch,risk,gov,pre app; class bres,bk,mkt,scan,ais,repo,auth infra; class api,ws,mw pres;
```

| Component | Role |
|-----------|------|
| **TradingService** | The orchestrator. `run_once` (deep analysis → plan), `check_triggers` (price hits entry → act), `check_exits` (manage/close), `execute` (the order path). |
| **Watcher** | The multi-tenant loop: every tick, watch prices for all auto-trading accounts; infrequently, run deep analysis. See [[Order-Execution]]. |
| **risk / governor / preflight** | Pure decision modules — the safety rails. |
| **BrokerResolver** | Maps `account_id` → the right per-tenant `Broker` with that tenant's credentials. |
| **MomentumScanner / BitkubMarket** | Build & filter the tradable universe (excludes broker coins — [[Broker-Integration]]). |
| **AiSidecar client** | The `AiEngine` adapter that calls the Python layer. |

## AI Layer (Python)

```mermaid
flowchart LR
    inp[/analyze request/] --> pipe[pipeline.py]
    pipe --> struct[market_structure<br/>RSI · ATR · ADX · ER · regime]
    pipe --> council
    subgraph council [council = agents/*]
        tech[technical]
        ml[trend_ml]
        fin[finbert]
        cry[cryptobert]
        nw[news + veto]
    end
    council --> agg[aggregator.py<br/>weighted vote + thresholds]
    agg --> judge[judge.py<br/>LLM or rule-based planner]
    struct --> judge
    judge --> disc[entry discipline<br/>regime-aware]
    disc --> out[/verdict + trace/]

    classDef ai fill:#1e3a5f,stroke:#4a90d9,color:#fff;
    class pipe,struct,agg,judge,disc ai;
```

| Component | Role |
|-----------|------|
| **pipeline.py** | Orchestrates one analysis: fetch candles → compute structure → run council → aggregate → judge → emit trace. |
| **agents/** | The five analysts; each returns `{action, confidence, reasoning}` or abstains. |
| **aggregator.py** | Weighted vote, asymmetric BUY/SELL thresholds, structural requirement, regime tagging. |
| **judge.py** | LLM judge (Ollama→cloud chain) **or** the deterministic `_plan_from_consensus`; then `_apply_entry_discipline`. See [[Entry-Strategy]]. |

Related: [[Analysis-Pipeline]] · [[Clean-Architecture]]
