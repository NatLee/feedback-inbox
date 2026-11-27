use std::{sync::Arc, time::Duration};

use anyhow::Result;
use sqlx::SqlitePool;

use crate::{auth::LoginLimiter, config::Config, notify::Mailer, ratelimit::RateLimiter};

pub type AppState = Arc<Inner>;

pub struct Inner {
    pub access: std::sync::OnceLock<access_kit::AccessKit>,
    pub db: SqlitePool,
    pub config: Config,
    pub limiter: LoginLimiter,
    /// 同一個 IP 一分鐘能送幾次表單
    pub submissions: RateLimiter,
    /// 沒設定 SMTP 時是 `None`
    pub mailer: Option<Mailer>,
    /// 送 webhook 用
    pub http: reqwest::Client,
}

impl Inner {
    pub fn new(db: SqlitePool, config: Config) -> Result<Self> {
        let mailer = config.smtp.as_ref().map(Mailer::new).transpose()?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("feedback-inbox/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            access: std::sync::OnceLock::new(),
            db,
            submissions: RateLimiter::new(config.rate_limit, Duration::from_secs(60)),
            config,
            limiter: LoginLimiter::default(),
            mailer,
            http,
        })
    }
}
