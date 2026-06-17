# Flow: Order Execution & the Watch Loop

The "watching" half of the system. Cheap and frequent price-watching triggers the expensive "thinking" only when it matters.

## The two-speed watch loop

```mermaid
flowchart TB
    tick([every monitor tick]) --> gov{governor<br/>state?}
    gov -->|halted/paused| exits[check_exits only<br/>can still cut losses]
    gov -->|active| both[check_triggers + check_exits]
    both --> deep{deep interval<br/>elapsed?}
    exits --> deep
    deep -->|no| tick
    deep -->|yes| scan[scan universe<br/>+ held positions]
    scan --> analyse[run_once per symbol<br/>= deep analysis → plan]
    analyse --> tick
    classDef hot fill:#1e3a5f,stroke:#4a90d9,color:#fff;
    class both,exits,analyse hot;
```

- **Monitor (frequent, no AI):** `check_triggers` (did a pending plan's entry get hit?) and `check_exits` (manage/close open positions).
- **Deep (infrequent, AI):** build the universe (scanner + watchlist + **held positions, always re-evaluated**), run [[Analysis-Pipeline]] per symbol, create/adjust plans.
- **Anti-churn:** a symbol just exited gets a 30-minute re-entry cooldown.

## The order path (`execute`)

Every order — whether an immediate market entry, a triggered pending entry, or an exit — funnels through one guarded path:

```mermaid
sequenceDiagram
    autonumber
    participant Svc as TradingService.execute
    participant Risk as risk::evaluate
    participant Pre as preflight::check_buy
    participant Br as Broker (Bitkub)
    participant DB as ledger

    Svc->>Risk: BUY? cap by cash / max positions / daily loss
    alt blocked or 0 cash (retried once)
        Risk-->>Svc: RiskBlocked → alert, stop
    end
    Svc->>Pre: amount vs cash, min order, price>0
    alt definitely-fail
        Pre-->>Svc: Block → alert, stop (no API call)
    else shrink-to-fit
        Pre-->>Svc: Shrink to 99.x% cash → alert, continue
    end
    Svc->>Br: place_order (market)
    Note over Br: source=="broker"? → reject early (error 61)<br/>else HMAC POST place-bid
    alt filled
        Br-->>Svc: order id + filled amount/price
        Svc->>DB: record TradeRecord(filled) + realized P&L
    else failed
        Br-->>Svc: broker error
        Svc->>DB: record TradeRecord(failed, note=reason)
        Svc->>DB: alert(order_failed, reason)
    end
```

## The safety rails (defence in depth)

| Layer | Module | Rejects when… | Outcome |
|-------|--------|---------------|---------|
| **Governor** | `governor` | paused / daily-loss hit / no slots / no cash | state shown in UI; no entries |
| **Risk** | `risk` | over max positions / over daily loss / 0 cash (retried) | `RiskBlocked` + alert |
| **Preflight** | `preflight` | amount ≤ 0, below min, price unreadable, cash short | `Block` (no API call) or `Shrink` to fit |
| **Broker guard** | `bitkub` | pair inactive, frozen, **broker coin (error 61)** | clear error before/after submit, recorded as `failed` |

Because the universe is now filtered to tradable **exchange** coins ([[Broker-Integration]]), the broker-coin rejection is a backstop, not the common case.

## Failure is first-class

A failed order is **never silent**: it's persisted with `status=failed` and a human-readable `note` (the exact broker reason), surfaced both in the Trades view and the Alerts stream. A pending plan whose confirmed buy fails is **cancelled** (not left to re-loop every tick).

Related: [[Entry-Strategy]] · [[Position-Management]] · [[Deployment-and-Security]]
