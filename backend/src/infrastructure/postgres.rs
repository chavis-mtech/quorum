//! PostgreSQL adapters — HistoryRepository + SecretStore + UserStore + AccountStore
//!
//! Uses runtime queries (not macros) so the crate can be built without a live DB at compile time

use async_trait::async_trait;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use crate::domain::models::{
    Account, AccountKind, Action, BrokerCredentials, DecisionRecord, ReportSummary, SecretMeta,
    TradingMode, User,
};
use crate::domain::ports::{
    AccountStore, DomainError, DomainResult, HistoryRepository, SecretStore, UserStore,
};

#[derive(Clone)]
pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    /// Run migrations from the db/migrations folder
    pub async fn migrate(&self) -> anyhow::Result<()> {
        sqlx::migrate!("../db/migrations").run(&self.pool).await?;
        Ok(())
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Set the real password for the default user if it is still the placeholder 'SETME'.
    /// Returns Some(email) if the password was just set (so the caller can log the credential)
    pub async fn finalize_default_user(
        &self,
        password_hash: &str,
    ) -> anyhow::Result<Option<String>> {
        let row = sqlx::query(
            "UPDATE users SET password_hash=$1, updated_at=now()
             WHERE id=1 AND password_hash='SETME' RETURNING email",
        )
        .bind(password_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.get::<String, _>("email")))
    }
}

fn mode_str(m: TradingMode) -> &'static str {
    match m {
        TradingMode::Paper => "paper",
        TradingMode::Live => "live",
        TradingMode::SignalOnly => "signal-only",
    }
}

fn parse_mode(s: &str) -> TradingMode {
    match s {
        "live" => TradingMode::Live,
        "paper" => TradingMode::Paper,
        _ => TradingMode::SignalOnly,
    }
}

fn row_to_record(row: &sqlx::postgres::PgRow) -> DecisionRecord {
    DecisionRecord {
        id: row.get("id"),
        account_id: row.get("account_id"),
        symbol: row.get("symbol"),
        quote: row.get("quote"),
        mode: parse_mode(row.get::<String, _>("mode").as_str()),
        final_action: Action::parse(row.get::<String, _>("final_action").as_str()),
        consensus_action: Action::parse(row.get::<String, _>("consensus_action").as_str()),
        consensus_confidence: row.get("consensus_confidence"),
        agreement: row.get::<i32, _>("agreement") as u32,
        voted: row.get::<i32, _>("voted") as u32,
        vetoed: row.get("vetoed"),
        judge_engine: row.get("judge_engine"),
        judge_reasoning: row.get("judge_reasoning"),
        last_price: row.get("last_price"),
        executed: row.get("executed"),
        note: row.get("note"),
        created_at: row.get("created_at"),
    }
}

fn row_to_user(r: &sqlx::postgres::PgRow) -> User {
    User {
        id: r.get("id"),
        email: r.get("email"),
        display_name: r.get("display_name"),
        role: r.get("role"),
        password_hash: r.get("password_hash"),
        created_at: r.get("created_at"),
    }
}

fn row_to_account(r: &sqlx::postgres::PgRow) -> Account {
    Account {
        id: r.get("id"),
        user_id: r.get("user_id"),
        kind: AccountKind::parse(r.get::<String, _>("kind").as_str()),
        name: r.get("name"),
        base_quote: r.get("base_quote"),
        created_at: r.get("created_at"),
    }
}

// ================= HistoryRepository (scoped) =================

#[async_trait]
impl HistoryRepository for PgStore {
    async fn save_decision(&self, r: &DecisionRecord) -> DomainResult<i64> {
        let id: i64 = sqlx::query(
            r#"INSERT INTO decisions
               (account_id, symbol, quote, mode, final_action, consensus_action, consensus_confidence,
                agreement, voted, vetoed, judge_engine, judge_reasoning, last_price,
                executed, note, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
               RETURNING id"#,
        )
        .bind(r.account_id)
        .bind(&r.symbol)
        .bind(&r.quote)
        .bind(mode_str(r.mode))
        .bind(r.final_action.as_str())
        .bind(r.consensus_action.as_str())
        .bind(r.consensus_confidence)
        .bind(r.agreement as i32)
        .bind(r.voted as i32)
        .bind(r.vetoed)
        .bind(&r.judge_engine)
        .bind(&r.judge_reasoning)
        .bind(r.last_price)
        .bind(r.executed)
        .bind(&r.note)
        .bind(r.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?
        .get("id");
        Ok(id)
    }

    async fn save_analysis_json(
        &self,
        decision_id: i64,
        raw: &serde_json::Value,
    ) -> DomainResult<()> {
        sqlx::query("UPDATE decisions SET raw_analysis = $1 WHERE id = $2")
            .bind(raw)
            .bind(decision_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(())
    }

    async fn mark_executed(&self, decision_id: i64, note: &str) -> DomainResult<()> {
        sqlx::query("UPDATE decisions SET executed=TRUE, note=$2 WHERE id=$1")
            .bind(decision_id)
            .bind(note)
            .execute(&self.pool)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(())
    }

    async fn recent_decisions(
        &self,
        account_id: i64,
        limit: i64,
    ) -> DomainResult<Vec<DecisionRecord>> {
        let rows = sqlx::query(
            "SELECT * FROM decisions WHERE account_id=$1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(account_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(rows.iter().map(row_to_record).collect())
    }

    async fn decisions_for_symbol(
        &self,
        account_id: i64,
        symbol: &str,
        limit: i64,
    ) -> DomainResult<Vec<DecisionRecord>> {
        let rows = sqlx::query(
            "SELECT * FROM decisions WHERE account_id=$1 AND symbol=$2 ORDER BY created_at DESC LIMIT $3",
        )
        .bind(account_id)
        .bind(symbol)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(rows.iter().map(row_to_record).collect())
    }

    async fn decision_analysis(&self, account_id: i64, id: i64) -> DomainResult<serde_json::Value> {
        let row = sqlx::query("SELECT raw_analysis FROM decisions WHERE id=$1 AND account_id=$2")
            .bind(id)
            .bind(account_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?
            .ok_or_else(|| DomainError::NotFound(format!("decision {id}")))?;
        Ok(row
            .get::<Option<serde_json::Value>, _>("raw_analysis")
            .unwrap_or(serde_json::Value::Null))
    }

    async fn report_summary(&self, account_id: i64) -> DomainResult<ReportSummary> {
        let row = sqlx::query(
            r#"SELECT
                 COUNT(*)                                          AS total,
                 COUNT(*) FILTER (WHERE executed)                  AS executed,
                 COUNT(*) FILTER (WHERE final_action = 'BUY')      AS buy,
                 COUNT(*) FILTER (WHERE final_action = 'SELL')     AS sell,
                 COUNT(*) FILTER (WHERE final_action = 'HOLD')     AS hold,
                 COUNT(*) FILTER (WHERE vetoed)                    AS vetoed,
                 COALESCE(AVG(consensus_confidence), 0)            AS avg_conf,
                 COUNT(DISTINCT symbol)                            AS symbols
               FROM decisions WHERE account_id=$1"#,
        )
        .bind(account_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(ReportSummary {
            total_decisions: row.get("total"),
            executed: row.get("executed"),
            buy: row.get("buy"),
            sell: row.get("sell"),
            hold: row.get("hold"),
            vetoed: row.get("vetoed"),
            avg_confidence: row.get("avg_conf"),
            symbols_tracked: row.get("symbols"),
        })
    }
}

// ================= SecretStore (per-user) =================

/// Masked preview of the api key — reveals the first 6 + last 4 chars when long enough (helps
/// the user confirm it is the same key). Short keys reveal only the last 2 chars; the full key
/// is never sent out.
fn mask_key(s: &str) -> String {
    let s = s.trim();
    let n = s.chars().count();
    if n == 0 {
        return String::new();
    }
    if n <= 8 {
        let tail: String = s.chars().skip(n.saturating_sub(2)).collect();
        return format!("{}{tail}", "•".repeat(n.saturating_sub(2)));
    }
    let head: String = s.chars().take(6).collect();
    let tail: String = s.chars().skip(n - 4).collect();
    format!("{head}…{tail}")
}

/// Masked tail of the secret — reveals only the last 4 chars (secrets have no meaningful prefix)
fn mask_tail(s: &str) -> String {
    let s = s.trim();
    let n = s.chars().count();
    if n == 0 {
        return String::new();
    }
    if n <= 4 {
        return "•".repeat(n);
    }
    let tail: String = s.chars().skip(n - 4).collect();
    format!("••••{tail}")
}

#[async_trait]
impl SecretStore for PgStore {
    async fn get(&self, user_id: i64, name: &str) -> DomainResult<Option<BrokerCredentials>> {
        let row = sqlx::query(
            "SELECT broker, api_key, api_secret FROM broker_credentials WHERE user_id=$1 AND broker=$2",
        )
        .bind(user_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Secret(e.to_string()))?;
        Ok(row.map(|r| BrokerCredentials {
            broker: r.get("broker"),
            api_key: r.get("api_key"),
            api_secret: r.get("api_secret"),
        }))
    }

    async fn set(&self, user_id: i64, c: &BrokerCredentials) -> DomainResult<()> {
        sqlx::query(
            r#"INSERT INTO broker_credentials (user_id, broker, api_key, api_secret, updated_at)
               VALUES ($1,$2,$3,$4, now())
               ON CONFLICT (user_id, broker)
               DO UPDATE SET api_key=$3, api_secret=$4, updated_at=now()"#,
        )
        .bind(user_id)
        .bind(&c.broker)
        .bind(&c.api_key)
        .bind(&c.api_secret)
        .execute(&self.pool)
        .await
        .map_err(|e| DomainError::Secret(e.to_string()))?;
        Ok(())
    }

    async fn exists(&self, user_id: i64, name: &str) -> DomainResult<bool> {
        let row =
            sqlx::query("SELECT 1 AS one FROM broker_credentials WHERE user_id=$1 AND broker=$2")
                .bind(user_id)
                .bind(name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| DomainError::Secret(e.to_string()))?;
        Ok(row.is_some())
    }

    async fn meta(&self, user_id: i64, name: &str) -> DomainResult<Option<SecretMeta>> {
        let row = sqlx::query(
            "SELECT api_key, api_secret, updated_at FROM broker_credentials WHERE user_id=$1 AND broker=$2",
        )
        .bind(user_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Secret(e.to_string()))?;
        Ok(row.map(|r| {
            let api_secret: String = r.get("api_secret");
            let has_secret = !api_secret.trim().is_empty();
            SecretMeta {
                api_key_hint: mask_key(&r.get::<String, _>("api_key")),
                api_secret_hint: if has_secret {
                    mask_tail(&api_secret)
                } else {
                    String::new()
                },
                has_secret,
                updated_at: r.get("updated_at"),
            }
        }))
    }
}

// ================= UserStore =================

#[async_trait]
impl UserStore for PgStore {
    async fn count(&self) -> DomainResult<i64> {
        let r = sqlx::query("SELECT COUNT(*) AS n FROM users")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(r.get("n"))
    }

    async fn create(
        &self,
        email: &str,
        password_hash: &str,
        display_name: &str,
        role: &str,
    ) -> DomainResult<User> {
        let r = sqlx::query(
            r#"INSERT INTO users (email, password_hash, display_name, role)
               VALUES ($1,$2,$3,$4) RETURNING *"#,
        )
        .bind(email)
        .bind(password_hash)
        .bind(display_name)
        .bind(role)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("duplicate") || e.to_string().contains("unique") {
                DomainError::Auth("This email is already in use".into())
            } else {
                DomainError::Repo(e.to_string())
            }
        })?;
        Ok(row_to_user(&r))
    }

    async fn by_email(&self, email: &str) -> DomainResult<Option<User>> {
        let r = sqlx::query("SELECT * FROM users WHERE lower(email)=lower($1)")
            .bind(email)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(r.as_ref().map(row_to_user))
    }

    async fn by_id(&self, id: i64) -> DomainResult<Option<User>> {
        let r = sqlx::query("SELECT * FROM users WHERE id=$1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(r.as_ref().map(row_to_user))
    }

    async fn update_profile(&self, id: i64, display_name: &str) -> DomainResult<()> {
        sqlx::query("UPDATE users SET display_name=$1, updated_at=now() WHERE id=$2")
            .bind(display_name)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(())
    }

    async fn update_password(&self, id: i64, password_hash: &str) -> DomainResult<()> {
        sqlx::query("UPDATE users SET password_hash=$1, updated_at=now() WHERE id=$2")
            .bind(password_hash)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(())
    }
}

// ================= AccountStore =================

#[async_trait]
impl AccountStore for PgStore {
    async fn create(&self, user_id: i64, kind: AccountKind, name: &str) -> DomainResult<Account> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        let acc = sqlx::query(
            r#"INSERT INTO accounts (user_id, kind, name) VALUES ($1,$2,$3) RETURNING *"#,
        )
        .bind(user_id)
        .bind(kind.as_str())
        .bind(name)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            if e.to_string().contains("unique") || e.to_string().contains("duplicate") {
                DomainError::Repo("An account with this name already exists".into())
            } else {
                DomainError::Repo(e.to_string())
            }
        })?;
        let account = row_to_account(&acc);
        // seed settings (live accounts start at signal-only for safety)
        let default_mode = match kind {
            AccountKind::Paper => "paper",
            AccountKind::Live => "signal-only",
        };
        sqlx::query(
            "INSERT INTO account_settings (account_id, mode) VALUES ($1,$2) ON CONFLICT DO NOTHING",
        )
        .bind(account.id)
        .bind(default_mode)
        .execute(&mut *tx)
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        // Seed a simulated wallet for every account (live accounts use it to track results in
        // signal-only / paper mode); without this row GET /api/wallet returns 500 because
        // view() uses fetch_one
        sqlx::query("INSERT INTO account_wallet (account_id) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(account.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(account)
    }

    async fn for_user(&self, user_id: i64) -> DomainResult<Vec<Account>> {
        let rows = sqlx::query("SELECT * FROM accounts WHERE user_id=$1 ORDER BY id")
            .bind(user_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(rows.iter().map(row_to_account).collect())
    }

    async fn get(&self, account_id: i64) -> DomainResult<Option<Account>> {
        let r = sqlx::query("SELECT * FROM accounts WHERE id=$1")
            .bind(account_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(r.as_ref().map(row_to_account))
    }

    async fn delete(&self, user_id: i64, account_id: i64) -> DomainResult<()> {
        // scoped by user_id — user A cannot delete user B's account
        // ON DELETE CASCADE handles children (settings/wallet/positions/trades/decisions/plans/watch/alerts)
        let res = sqlx::query("DELETE FROM accounts WHERE id=$1 AND user_id=$2")
            .bind(account_id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        if res.rows_affected() == 0 {
            return Err(DomainError::NotFound(
                "Account not found or does not belong to you".into(),
            ));
        }
        Ok(())
    }

    async fn auto_trading(&self) -> DomainResult<Vec<Account>> {
        let rows = sqlx::query(
            r#"SELECT a.* FROM accounts a
               JOIN account_settings s ON s.account_id = a.id
               WHERE s.auto_trade = TRUE AND s.paused = FALSE
               ORDER BY a.id"#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(rows.iter().map(row_to_account).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{mask_key, mask_tail};

    #[test]
    fn mask_key_reveals_head_and_tail_when_long() {
        assert_eq!(mask_key("sk-or-v1-abcdef3f9a"), "sk-or-…3f9a");
    }

    #[test]
    fn mask_key_hides_most_of_short_keys() {
        assert_eq!(mask_key("abcd"), "••cd");
        assert_eq!(mask_key("12345678"), "••••••78");
    }

    #[test]
    fn mask_key_empty_is_empty() {
        assert_eq!(mask_key(""), "");
        assert_eq!(mask_key("   "), "");
    }

    #[test]
    fn mask_tail_shows_only_last_four() {
        assert_eq!(mask_tail("supersecretvalue1234"), "••••1234");
        assert_eq!(mask_tail("ab"), "••");
        assert_eq!(mask_tail(""), "");
    }
}
