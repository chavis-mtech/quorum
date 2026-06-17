//! Watch loop — monitors "all accounts with auto-trade enabled" (multi-tenant)
//!   - monitor (frequent, lightweight): watches prices → triggers entries per plan + closes positions per target/stop
//!   - deep (infrequent): deep AI analysis → creates/adjusts plans (quality > frequency)
//!
//! Accounts without auto-trade enabled are not watched (users can always trigger analysis manually via /api/analyze)

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::trading::TradingService;
use crate::domain::models::LiveEvent;
use crate::domain::ports::{AccountStore, EventSink, MarketScanner, SettingsStore, WatchlistStore};

pub struct Watcher {
    pub service: Arc<TradingService>,
    pub accounts: Arc<dyn AccountStore>,
    pub settings: Arc<dyn SettingsStore>,
    pub watch_store: Arc<dyn WatchlistStore>,
    pub scanner: Arc<dyn MarketScanner>,
    pub events: Arc<dyn EventSink>,
    /// Price watch interval (lightweight)
    pub monitor_interval: Duration,
    /// Deep analysis interval (heavyweight)
    pub deep_interval: Duration,
    /// Cooldown for assets that were analyzed but have no plan yet
    pub no_plan_cooldown: Duration,
}

impl Watcher {
    pub async fn run(self) {
        for _ in 0..15 {
            if self.service.ai.health().await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        let mut ticker = tokio::time::interval(self.monitor_interval);
        let mut last_deep: Option<Instant> = None;
        // cooldown per (account, symbol)
        let mut no_plan_until: HashMap<(i64, String), Instant> = HashMap::new();
        // remember last governor state per account to emit only on change
        let mut last_gov: HashMap<i64, String> = HashMap::new();

        loop {
            ticker.tick().await;

            let accounts = match self.accounts.auto_trading().await {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!("failed to load auto-trade accounts: {e}");
                    continue;
                }
            };
            if accounts.is_empty() {
                continue;
            }

            // ----- every tick: price watch (lightweight) for all accounts + governor control -----
            let mut blocked: HashMap<i64, bool> = HashMap::new();
            // symbols closed this tick → apply a re-entry cooldown so the bot does not immediately
            // re-buy a coin it just stopped out of (anti-churn / revenge-trade guard)
            let mut just_exited: Vec<(i64, String)> = Vec::new();
            for acc in &accounts {
                let settings = match self.settings.get(acc.id).await {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let gov = self.service.governor_state(acc.id).await;
                // notify when state changes (e.g. losses hit the ceiling → halted)
                if last_gov.get(&acc.id) != Some(&gov.state) {
                    self.events.publish(&LiveEvent::Governor {
                        governor: gov.clone(),
                    });
                    self.events.publish(&LiveEvent::Status {
                        message: gov.reason.clone(),
                        healthy: !gov.is_blocked(),
                    });
                    last_gov.insert(acc.id, gov.state.clone());
                }
                let is_halted = gov.state == "halted";
                blocked.insert(acc.id, gov.is_blocked());
                let closed = if is_halted {
                    // loss ceiling hit: no new entries, but existing positions can still be closed (cut loss)
                    self.service.check_exits(acc.id, &settings).await
                } else {
                    self.service.check_triggers(acc.id, &settings).await;
                    self.service.check_exits(acc.id, &settings).await
                };
                for sym in closed {
                    just_exited.push((acc.id, sym));
                }
            }
            // register the re-entry cooldown for anything just closed (30 min)
            let reentry_until = Instant::now() + Duration::from_secs(1800);
            for (aid, sym) in just_exited {
                no_plan_until.insert((aid, sym), reentry_until);
            }

            // ----- infrequent: deep analysis -----
            let due = last_deep.map_or(true, |t| t.elapsed() >= self.deep_interval);
            if !due {
                continue;
            }
            last_deep = Some(Instant::now());

            let now = Instant::now();
            no_plan_until.retain(|_, until| *until > now);

            for acc in &accounts {
                // halted/paused accounts → skip deep analysis (saves AI cost + makes the halt explicit)
                if blocked.get(&acc.id).copied().unwrap_or(false) {
                    continue;
                }
                let settings = match self.settings.get(acc.id).await {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let mut targets: BTreeSet<String> = self
                    .watch_store
                    .get_symbols(acc.id)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .collect();
                if settings.discovery_enabled {
                    match self.scanner.scan(settings.discovery_top_n as usize).await {
                        Ok(items) if !items.is_empty() => {
                            self.events.publish(&LiveEvent::Discovery {
                                items: items.clone(),
                            });
                            for it in items {
                                targets.insert(it.symbol);
                            }
                        }
                        Ok(_) => {}
                        Err(e) => tracing::warn!("market scan failed: {e}"),
                    }
                }

                let held = self.service.held_plan_symbols(acc.id).await;
                let tracked = self.service.active_plan_symbols(acc.id).await;
                let mut todo: Vec<String> = targets
                    .into_iter()
                    .filter(|s| !tracked.contains(s))
                    .filter(|s| {
                        no_plan_until
                            .get(&(acc.id, s.clone()))
                            .map_or(true, |until| *until <= now)
                    })
                    .collect();
                // ALWAYS re-evaluate currently-held positions so the AI keeps managing them
                // (adjust target/stop or exit when the thesis breaks) instead of buy-and-forget
                for s in &held {
                    if !todo.contains(s) {
                        todo.push(s.clone());
                    }
                }
                tracing::info!(
                    "account {} deep analysis for {} symbols ({} held positions re-evaluated)",
                    acc.id,
                    todo.len(),
                    held.len()
                );
                for sym in todo {
                    match self.service.run_once(acc.id, &sym).await {
                        Ok(_) => {
                            if self.no_plan_cooldown > Duration::ZERO
                                && !self.service.has_active_plan(acc.id, &sym).await
                            {
                                no_plan_until.insert(
                                    (acc.id, sym.clone()),
                                    Instant::now() + self.no_plan_cooldown,
                                );
                            }
                        }
                        Err(e) => tracing::warn!("analysis for {sym} failed: {e}"),
                    }
                }
            }
        }
    }
}
