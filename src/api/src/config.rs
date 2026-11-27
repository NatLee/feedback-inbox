//! 設定都來自環境變數，全部都有預設值。

use std::{env, path::PathBuf};

use anyhow::{bail, Context, Result};

pub const DEFAULT_BIND: &str = "0.0.0.0:8787";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmtpTls {
    /// 先用明文連線再升級（587）
    StartTls,
    /// 一開始就是 TLS（465）
    Tls,
    /// 不加密；只給同一台機器或內部網路上的測試用信箱
    None,
}

#[derive(Clone, Debug)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    /// 寄件人，例如 `Feedback Inbox <noreply@example.com>`
    pub from: String,
    pub tls: SmtpTls,
}

#[derive(Clone)]
pub struct EncryptionKey(pub [u8; 32]);
impl std::fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}
#[derive(Clone, Debug)]
pub struct WorkflowConfig {
    pub secret_key: Option<EncryptionKey>,
    pub secret_error: Option<String>,
    pub allowed_ollama_origins: Vec<String>,
    pub auto_send_enabled: bool,
    pub concurrency: usize,
}
impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            secret_key: None,
            secret_error: None,
            allowed_ollama_origins: vec!["http://127.0.0.1:11434".into()],
            auto_send_enabled: false,
            concurrency: 2,
        }
    }
}
impl WorkflowConfig {
    fn from_env() -> Self {
        use base64::Engine;
        let mut config = Self::default();
        if let Some(value) = var("FI_AI_SECRET_KEY") {
            match base64::engine::general_purpose::STANDARD
                .decode(value.trim())
                .ok()
                .and_then(|v| v.try_into().ok())
            {
                Some(key) => config.secret_key = Some(EncryptionKey(key)),
                None => {
                    config.secret_error =
                        Some("FI_AI_SECRET_KEY 必須是 base64 編碼的 32 位元組金鑰".into())
                }
            }
        }
        if let Some(value) = var("FI_AI_ALLOWED_OLLAMA_ORIGINS") {
            config.allowed_ollama_origins = value.split(',').filter_map(origin_of).collect();
        }
        config.auto_send_enabled = var("FI_AI_AUTO_SEND_ENABLED").is_some_and(|v| truthy(&v));
        config.concurrency = var("FI_AI_CONCURRENCY")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(2)
            .clamp(1, 8);
        config
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: String,
    pub database_path: PathBuf,
    pub static_dir: PathBuf,
    /// 外部看到的網址（例如 `https://inbox.example.com`），沒有結尾的斜線；
    /// 通知裡的連結和嵌入程式碼會用到。
    pub public_url: Option<String>,
    /// `public_url` 的 origin；用來判斷寫入請求是不是同源。
    pub public_origin: Option<String>,
    /// 登入 cookie 要不要加 `Secure`。沒設定時看 `FI_PUBLIC_URL` 是不是 https。
    pub cookie_secure: bool,
    /// 前面有反向代理時設成 true，才會從 `X-Forwarded-For` 讀訪客的 IP。
    pub trust_proxy: bool,
    /// 同一個 IP 每分鐘最多送幾次表單；0 是不限制。
    pub rate_limit: u32,
    /// 沒設定 `FI_SMTP_HOST` 就不寄信。
    pub smtp: Option<SmtpConfig>,
    pub workflow: WorkflowConfig,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let data_dir = PathBuf::from(var("FI_DATA_DIR").unwrap_or_else(|| "data".into()));
        let legacy = var("FI_PUBLIC_URL");
        let access = var("ACCESS_PUBLIC_URL");
        if let (Some(left), Some(right)) = (&legacy, &access) {
            anyhow::ensure!(
                origin_of(left) == origin_of(right),
                "FI_PUBLIC_URL and ACCESS_PUBLIC_URL must have the same origin"
            );
        }
        let public_url = legacy
            .or(access)
            .map(|u| u.trim().trim_end_matches('/').to_owned());
        let cookie_secure = match var("FI_COOKIE_SECURE") {
            Some(v) => truthy(&v),
            None => public_url
                .as_deref()
                .is_some_and(|u| u.starts_with("https://")),
        };
        Ok(Self {
            bind: var("FI_BIND").unwrap_or_else(|| DEFAULT_BIND.into()),
            database_path: var("FI_DATABASE_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| data_dir.join("feedback-inbox.db")),
            static_dir: PathBuf::from(
                var("FI_STATIC_DIR").unwrap_or_else(|| "src/web/dist".into()),
            ),
            public_origin: public_url.as_deref().and_then(origin_of),
            public_url,
            cookie_secure,
            trust_proxy: var("FI_TRUST_PROXY").is_some_and(|v| truthy(&v)),
            rate_limit: parse_var("FI_RATE_LIMIT", 10)?,
            smtp: smtp_from_env()?,
            workflow: WorkflowConfig::from_env(),
        })
    }
}

fn smtp_from_env() -> Result<Option<SmtpConfig>> {
    let Some(host) = var("FI_SMTP_HOST") else {
        return Ok(None);
    };
    let tls = match var("FI_SMTP_TLS").as_deref().map(str::trim) {
        None | Some("starttls") => SmtpTls::StartTls,
        Some("tls") => SmtpTls::Tls,
        Some("none") => SmtpTls::None,
        Some(other) => bail!("FI_SMTP_TLS 只能是 starttls、tls 或 none：{other}"),
    };
    let default_port = match tls {
        SmtpTls::StartTls => 587,
        SmtpTls::Tls => 465,
        SmtpTls::None => 25,
    };
    let Some(from) = var("FI_SMTP_FROM") else {
        bail!("設了 FI_SMTP_HOST 就要設 FI_SMTP_FROM（寄件人信箱）");
    };
    Ok(Some(SmtpConfig {
        host: host.trim().to_owned(),
        port: parse_var("FI_SMTP_PORT", default_port)?,
        username: var("FI_SMTP_USERNAME"),
        password: var("FI_SMTP_PASSWORD"),
        from: from.trim().to_owned(),
        tls,
    }))
}

fn truthy(v: &str) -> bool {
    matches!(v.trim(), "1" | "true" | "yes")
}

/// `https://host:port/path` → `https://host:port`
pub fn origin_of(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    let origin = parsed.origin().ascii_serialization();
    if !origin
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b":/.-_[]".contains(&b))
    {
        return None;
    }
    Some(origin)
}

/// 讀環境變數；沒設定或只有空白都當成沒設定。
pub fn var(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn parse_var<T: std::str::FromStr>(name: &str, default: T) -> Result<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match var(name) {
        Some(v) => v
            .trim()
            .parse()
            .with_context(|| format!("{name} 的值不合法：{v}")),
        None => Ok(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_cannot_inject_csp_directives() {
        assert_eq!(origin_of("https://example.com;script-src"), None);
        assert_eq!(origin_of("https://example.com'"), None);
        assert_eq!(
            origin_of("https://example.com:443/path"),
            Some("https://example.com".into())
        );
    }

    #[test]
    fn origin_drops_the_path() {
        assert_eq!(
            origin_of("https://WWW.example.com:8443/contact?x=1"),
            Some("https://www.example.com:8443".into())
        );
        assert_eq!(origin_of("not a url"), None);
        assert_eq!(origin_of("javascript://alert(1)"), None);
        assert_eq!(origin_of("https://user@evil.example"), None);
    }
}
