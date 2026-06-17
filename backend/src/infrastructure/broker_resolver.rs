//! PgBrokerResolver — resolves account_id → Broker with the credentials of the account owner
//!
//! Multi-broker: selects adapter based on `account_settings.broker` (bitkub | binance | ...)
//!   - To add a new broker: write an adapter in infrastructure/ → add one arm in `build()`
//!   - secrets are namespaced per broker (`secrets.get(user_id, kind.secret_name())`)
//!     → a user can store keys for multiple brokers simultaneously without collision
//!
//! caches broker per (user, broker kind) to reuse the http client
//! but refreshes credentials on every resolve — user A can never use user B's key

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::binance::BinanceBroker;
use super::bitkub::BitkubBroker;
use crate::domain::models::{BrokerCredentials, BrokerKind};
use crate::domain::ports::{
    AccountStore, Broker, BrokerResolver, DomainError, DomainResult, SecretStore, SettingsStore,
};

/// cached adapter — internal enum to allow typed set_credentials calls
enum CachedBroker {
    Bitkub(Arc<BitkubBroker>),
    Binance(Arc<BinanceBroker>),
}

impl CachedBroker {
    async fn refresh(&self, creds: Option<BrokerCredentials>) -> Arc<dyn Broker> {
        match self {
            CachedBroker::Bitkub(b) => {
                b.set_credentials(creds).await;
                b.clone() as Arc<dyn Broker>
            }
            CachedBroker::Binance(b) => {
                b.set_credentials(creds).await;
                b.clone() as Arc<dyn Broker>
            }
        }
    }
}

fn build(kind: BrokerKind, creds: Option<BrokerCredentials>) -> CachedBroker {
    match kind {
        BrokerKind::Bitkub => CachedBroker::Bitkub(Arc::new(BitkubBroker::new(creds))),
        BrokerKind::Binance => CachedBroker::Binance(Arc::new(BinanceBroker::new(creds))),
    }
}

pub struct PgBrokerResolver {
    accounts: Arc<dyn AccountStore>,
    secrets: Arc<dyn SecretStore>,
    settings: Arc<dyn SettingsStore>,
    cache: RwLock<HashMap<(i64, BrokerKind), CachedBroker>>, // keyed by (user_id, broker kind)
}

impl PgBrokerResolver {
    pub fn new(
        accounts: Arc<dyn AccountStore>,
        secrets: Arc<dyn SecretStore>,
        settings: Arc<dyn SettingsStore>,
    ) -> Self {
        Self {
            accounts,
            secrets,
            settings,
            cache: RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl BrokerResolver for PgBrokerResolver {
    async fn resolve(&self, account_id: i64) -> DomainResult<Arc<dyn Broker>> {
        let account = self
            .accounts
            .get(account_id)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("account {account_id}")))?;
        // broker for this account — read settings; fallback to bitkub if unavailable (the ready default)
        let kind = self
            .settings
            .get(account_id)
            .await
            .map(|s| BrokerKind::parse(&s.broker))
            .unwrap_or(BrokerKind::Bitkub);
        if !kind.is_ready() {
            tracing::warn!(
                broker = kind.as_str(),
                account_id,
                "this broker is a skeleton — price fetching works but placing orders/reading balances will error explicitly"
            );
        }
        let creds = self
            .secrets
            .get(account.user_id, kind.secret_name())
            .await?;

        let key = (account.user_id, kind);
        // reuse the adapter for this (user, broker) if it exists (update credentials to the latest)
        {
            let cache = self.cache.read().await;
            if let Some(b) = cache.get(&key) {
                return Ok(b.refresh(creds).await);
            }
        }
        let entry = build(kind, creds.clone());
        let arc = entry.refresh(creds).await;
        self.cache.write().await.insert(key, entry);
        Ok(arc)
    }
}
