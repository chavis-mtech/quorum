//! Infrastructure layer — adapters that implement ports (DB, HTTP, broker, events)
pub mod ai_sidecar;
pub mod auth;
pub mod binance;
pub mod bitkub;
pub mod broker_resolver;
pub mod events;
pub mod market;
pub mod postgres;
pub mod postgres_trading;
pub mod qpack;
pub mod scanner;
