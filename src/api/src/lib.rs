//! 網站聯絡表單的收件匣：每個網站一個收表單的網址，
//! 新訊息寄信或送 webhook 通知，大家在同一個收件匣裡追蹤處理進度。

pub mod auth;
pub mod config;
pub mod db;
pub mod demo;
pub mod error;
pub mod intake;
pub mod notify;
pub mod ratelimit;
pub mod routes;
pub mod state;

use std::sync::Arc;

use anyhow::Result;
use axum::Router;

pub use config::Config;
pub use state::{AppState, Inner};

/// 開資料庫（必要時建立並套用 migration）。
pub async fn build_state(config: Config) -> Result<AppState> {
    let db = db::connect(&config.database_path).await?;
    Ok(Arc::new(Inner::new(db, config)?))
}

pub fn app(state: AppState) -> Router {
    routes::router(state)
}
