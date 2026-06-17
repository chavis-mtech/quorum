//! AppState — aggregates dependencies required by handlers (composed in main.rs)

use std::sync::Arc;

use crate::application::trading::TradingService;
use crate::domain::ports::{
    AccountStore, AlertStore, Broker, HistoryRepository, MarketData, MarketScanner, PaperWallet,
    PlanRepository, SecretStore, SettingsStore, TradeRepository, UserStore, WatchlistStore,
};
use crate::infrastructure::auth::Auth;
use crate::infrastructure::events::BroadcastSink;

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<TradingService>,
    pub repo: Arc<dyn HistoryRepository>,
    pub trades: Arc<dyn TradeRepository>,
    pub plans: Arc<dyn PlanRepository>,
    pub secrets: Arc<dyn SecretStore>,
    pub settings: Arc<dyn SettingsStore>,
    pub wallet: Arc<dyn PaperWallet>,
    pub market: Arc<dyn MarketData>,
    pub scanner: Arc<dyn MarketScanner>,
    pub live_broker: Arc<dyn Broker>,
    pub watch_store: Arc<dyn WatchlistStore>,
    pub users: Arc<dyn UserStore>,
    pub accounts: Arc<dyn AccountStore>,
    pub alerts: Arc<dyn AlertStore>,
    pub auth: Arc<Auth>,
    pub events: BroadcastSink,
}
