//! 有新訊息時的通知：寄信給負責這個網站的人，以及送到網站設定的 webhook。

use std::{future::Future, time::Duration};

use anyhow::{anyhow, Context, Result};
use futures_util::future::join_all;
use lettre::{
    message::{header::ContentType, Mailbox, MultiPart, SinglePart},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    config::{SmtpConfig, SmtpTls},
    db,
    intake::{truncate, Extra},
    state::AppState,
};

const TIMEOUT: Duration = Duration::from_secs(15);
/// 一個目標從開始到結束最多等多久
const DEADLINE: Duration = Duration::from_secs(20);

/// 通知的內容。
#[derive(Debug, Clone)]
pub struct Notice {
    pub site_name: String,
    pub feedback_id: i64,
    pub name: String,
    pub email: String,
    pub phone: String,
    pub company: String,
    pub message: String,
    pub extra: Vec<Extra>,
    pub created_at: String,
    /// 這則訊息在收件匣裡的網址（有設 `FI_PUBLIC_URL` 才有）
    pub link: Option<String>,
}

/// 一次通知的結果。
#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub channel: &'static str,
    pub target: String,
    pub ok: bool,
    pub detail: String,
}

pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl Mailer {
    pub fn new(config: &SmtpConfig) -> Result<Self> {
        let from: Mailbox = config
            .from
            .parse()
            .with_context(|| format!("FI_SMTP_FROM 不是合法的信箱：{}", config.from))?;
        let builder = match config.tls {
            SmtpTls::StartTls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)?
            }
            SmtpTls::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)?,
            SmtpTls::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host),
        };
        let mut builder = builder.port(config.port).timeout(Some(TIMEOUT));
        if let (Some(username), Some(password)) = (&config.username, &config.password) {
            builder = builder.credentials(Credentials::new(username.clone(), password.clone()));
        }
        Ok(Self {
            transport: builder.build(),
            from,
        })
    }

    pub async fn send(&self, to: &str, notice: &Notice) -> Result<()> {
        let to: Mailbox = to.parse().map_err(|_| anyhow!("收件信箱的格式不對"))?;
        let mut message = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(subject(notice));
        // 直接按「回覆」就是回給留言的人
        if let Ok(reply_to) = notice.email.parse::<Mailbox>() {
            message = message.reply_to(reply_to);
        }
        let message = message.multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(text_body(notice)),
                )
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(html_body(notice)),
                ),
        )?;
        self.transport.send(message).await?;
        Ok(())
    }
}

/// 標題裡不能有換行。
fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// 「王小明（範例科技）」；都沒填就是「訪客」。
fn sender(notice: &Notice) -> String {
    let name = one_line(&notice.name);
    let company = one_line(&notice.company);
    match (name.is_empty(), company.is_empty()) {
        (false, false) => format!("{name}（{company}）"),
        (false, true) => name,
        (true, false) => company,
        (true, true) => "訪客".into(),
    }
}

pub fn subject(notice: &Notice) -> String {
    truncate(
        &format!(
            "[{}] {} 的新訊息",
            one_line(&notice.site_name),
            sender(notice)
        ),
        150,
    )
}

/// 有填的欄位，照固定的順序。
fn rows(notice: &Notice) -> Vec<(&str, &str)> {
    let mut rows = vec![
        ("姓名", notice.name.as_str()),
        ("公司", notice.company.as_str()),
        ("Email", notice.email.as_str()),
        ("電話", notice.phone.as_str()),
    ];
    rows.extend(
        notice
            .extra
            .iter()
            .map(|e| (e.name.as_str(), e.value.as_str())),
    );
    rows.retain(|(_, value)| !value.is_empty());
    rows
}

pub fn text_body(notice: &Notice) -> String {
    let mut out = format!("{} 收到一則新訊息\n\n", notice.site_name);
    for (label, value) in rows(notice) {
        out.push_str(&format!("{label}：{value}\n"));
    }
    if !notice.message.is_empty() {
        out.push_str(&format!("\n{}\n", notice.message));
    }
    if let Some(link) = &notice.link {
        out.push_str(&format!("\n在收件匣裡打開：{link}\n"));
    }
    out
}

pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

pub fn html_body(notice: &Notice) -> String {
    let mut table = String::new();
    for (label, value) in rows(notice) {
        table.push_str(&format!(
            "<tr><td style=\"padding:6px 16px 6px 0;color:#71717a;white-space:nowrap;vertical-align:top\">{}</td>\
             <td style=\"padding:6px 0;color:#18181b\">{}</td></tr>",
            escape_html(label),
            escape_html(value)
        ));
    }
    let message = if notice.message.is_empty() {
        String::new()
    } else {
        format!(
            "<div style=\"margin-top:16px;padding:14px 16px;background:#f4f4f5;border-radius:10px;\
             color:#18181b;white-space:pre-wrap;line-height:1.6\">{}</div>",
            escape_html(&notice.message)
        )
    };
    let button = match &notice.link {
        Some(link) => format!(
            "<p style=\"margin:24px 0 0\"><a href=\"{}\" style=\"display:inline-block;padding:9px 16px;\
             background:#0d9488;color:#ffffff;border-radius:8px;text-decoration:none;font-weight:600\">\
             在收件匣裡打開</a></p>",
            escape_html(link)
        ),
        None => String::new(),
    };
    format!(
        "<!doctype html><html><body style=\"margin:0;padding:24px;background:#fafafa;\
         font-family:-apple-system,'Segoe UI','Noto Sans TC','Microsoft JhengHei',sans-serif;font-size:14px\">\
         <div style=\"max-width:560px;margin:0 auto;padding:24px;background:#ffffff;\
         border:1px solid #e4e4e7;border-radius:14px\">\
         <p style=\"margin:0 0 4px;color:#0d9488;font-size:12px;font-weight:600\">{site}</p>\
         <h1 style=\"margin:0 0 16px;font-size:18px;color:#18181b\">{sender} 的新訊息</h1>\
         <table style=\"border-collapse:collapse\">{table}</table>{message}{button}\
         </div></body></html>",
        site = escape_html(&notice.site_name),
        sender = escape_html(&sender(notice)),
    )
}

#[derive(Debug, PartialEq, Eq)]
enum WebhookKind {
    Discord,
    Slack,
    Generic,
}

fn webhook_kind(url: &str) -> WebhookKind {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or_default();
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_ascii_lowercase();
    let is = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    if (is("discord.com") || is("discordapp.com")) && path.starts_with("api/webhooks/") {
        WebhookKind::Discord
    } else if is("hooks.slack.com") {
        WebhookKind::Slack
    } else {
        WebhookKind::Generic
    }
}

/// webhook 的內容。Discord 和 Slack 的網址會自動用它們的格式，其他的送完整的 JSON。
pub fn webhook_payload(url: &str, notice: &Notice) -> Value {
    let title = format!("{} 的新訊息", sender(notice));
    match webhook_kind(url) {
        WebhookKind::Discord => {
            let fields: Vec<Value> = rows(notice)
                .into_iter()
                .take(25)
                .map(|(label, value)| {
                    json!({ "name": truncate(label, 250), "value": truncate(value, 1000), "inline": true })
                })
                .collect();
            json!({
                "username": "Feedback Inbox",
                // 留言裡的 @everyone 不會真的通知到人
                "allowed_mentions": { "parse": [] },
                "embeds": [{
                    "author": { "name": truncate(&notice.site_name, 250) },
                    "title": truncate(&title, 250),
                    "url": notice.link,
                    "description": truncate(&notice.message, 3500),
                    "fields": fields,
                    "timestamp": notice.created_at,
                    "color": 0x0d9488,
                }],
            })
        }
        WebhookKind::Slack => {
            let mut text = format!("*[{}] {}*\n", notice.site_name, title);
            for (label, value) in rows(notice) {
                text.push_str(&format!("{label}：{value}\n"));
            }
            if !notice.message.is_empty() {
                text.push_str(&format!("\n{}\n", truncate(&notice.message, 3000)));
            }
            if let Some(link) = &notice.link {
                text.push_str(&format!("\n<{link}|在收件匣裡打開>"));
            }
            json!({ "text": text })
        }
        WebhookKind::Generic => json!({
            "event": "feedback.created",
            "site": notice.site_name,
            "feedback": {
                "id": notice.feedback_id,
                "name": notice.name,
                "email": notice.email,
                "phone": notice.phone,
                "company": notice.company,
                "message": notice.message,
                "extra": notice.extra,
                "created_at": notice.created_at,
            },
            "url": notice.link,
        }),
    }
}

async fn send_webhook(client: &reqwest::Client, url: &str, notice: &Notice) -> Result<()> {
    let response = client
        .post(url)
        .json(&webhook_payload(url, notice))
        .send()
        .await
        // reqwest 的錯誤訊息會帶完整網址，而 webhook 的網址本身就是密碼
        .map_err(|e| anyhow!("連不上：{}", e.without_url()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!("對方回應 {status}"));
    }
    Ok(())
}

/// 只留網址的 host，其餘的遮掉（webhook 的網址本身就是密碼）。
pub fn mask_url(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let host = rest.split(['/', '?']).next().unwrap_or_default();
            format!("{scheme}://{host}/…")
        }
        None => "…".into(),
    }
}

pub struct Targets {
    /// 要收信的信箱
    pub emails: Vec<String>,
    pub webhook_url: Option<String>,
}

/// 這個網站的通知要送去哪。
pub async fn targets(state: &AppState, site_id: i64) -> Result<Targets> {
    let (notify_email, webhook_url): (bool, String) =
        sqlx::query_as("SELECT notify_email, webhook_url FROM sites WHERE id = ?")
            .bind(site_id)
            .fetch_one(&state.db)
            .await?;
    let emails = if notify_email {
        sqlx::query_scalar(
            "SELECT u.email FROM site_members m JOIN users u ON u.id = m.user_id
             WHERE m.site_id = ? AND u.disabled = 0 AND u.email != '' ORDER BY u.id",
        )
        .bind(site_id)
        .fetch_all(&state.db)
        .await?
    } else {
        Vec::new()
    };
    Ok(Targets {
        emails,
        webhook_url: Some(webhook_url).filter(|u| !u.is_empty()),
    })
}

/// 等一個目標送完，最多等 [`DEADLINE`]：對方收了連線卻不回話的時候，函式庫自己的逾時不一定擋得住。
async fn bounded(sending: impl Future<Output = Result<()>>) -> Result<()> {
    match tokio::time::timeout(DEADLINE, sending).await {
        Ok(result) => result,
        Err(_) => Err(anyhow!("等了 {} 秒沒有回應", DEADLINE.as_secs())),
    }
}

fn outcome(channel: &'static str, target: String, result: Result<()>) -> Outcome {
    Outcome {
        channel,
        target,
        ok: result.is_ok(),
        detail: result.err().map(|e| format!("{e:#}")).unwrap_or_default(),
    }
}

/// 把通知送出去，回傳每一個目標的結果。沒設定 SMTP 時不寄信（也不算失敗）。
/// 每個目標各送各的，一個卡住不會拖到其他的。
pub async fn send(state: &AppState, targets: &Targets, notice: &Notice) -> Vec<Outcome> {
    let emails = async {
        let Some(mailer) = &state.mailer else {
            return Vec::new();
        };
        join_all(targets.emails.iter().map(|email| async move {
            let result = bounded(mailer.send(email, notice)).await;
            outcome("email", email.clone(), result)
        }))
        .await
    };
    let webhook = async {
        let url = targets.webhook_url.as_ref()?;
        let result = bounded(send_webhook(&state.http, url, notice)).await;
        Some(outcome("webhook", mask_url(url), result))
    };
    let (mut outcomes, webhook) = tokio::join!(emails, webhook);
    outcomes.extend(webhook);
    outcomes
}

/// Snapshot one durable task per destination inside the intake transaction.
pub async fn enqueue(
    tx: &mut sqlx::SqliteConnection,
    site: i64,
    feedback: i64,
) -> crate::error::AppResult<()> {
    let (email, webhook): (bool, String) =
        sqlx::query_as("SELECT notify_email,webhook_url FROM sites WHERE id=?")
            .bind(site)
            .fetch_one(&mut *tx)
            .await?;
    if email {
        let targets:Vec<String>=sqlx::query_scalar("SELECT u.email FROM users u JOIN site_members m ON m.user_id=u.id WHERE m.site_id=? AND u.disabled=0 AND u.email!=''").bind(site).fetch_all(&mut *tx).await?;
        for target in targets {
            let hash = crate::auth::sha256_hex(&target);
            crate::jobs::enqueue(
                tx,
                site,
                "notify_email",
                json!({"feedback_id":feedback,"target":target}),
                &format!("notice:{feedback}:email:{hash}"),
            )
            .await?;
        }
    }
    if !webhook.is_empty() {
        let hash = crate::auth::sha256_hex(&webhook);
        crate::jobs::enqueue(
            tx,
            site,
            "notify_webhook",
            json!({"feedback_id":feedback,"target_hash":hash}),
            &format!("notice:{feedback}:webhook:{hash}"),
        )
        .await?;
    }
    Ok(())
}
#[derive(sqlx::FromRow)]
struct NoticeRow {
    id: i64,
    site_id: i64,
    site_name: String,
    name: String,
    email: String,
    phone: String,
    company: String,
    message: String,
    extra: String,
    created_at: String,
}
pub async fn execute(state: &AppState, claim: &crate::jobs::Claim) -> crate::error::AppResult<()> {
    use crate::error::AppError;
    let payload: Value = serde_json::from_str(&claim.payload_json)?;
    let id = payload["feedback_id"]
        .as_i64()
        .ok_or_else(|| AppError::bad("通知工作格式不符"))?;
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    crate::jobs::current(&mut tx, claim).await?;
    let row:Option<NoticeRow>=sqlx::query_as("SELECT f.id,f.site_id,s.name site_name,f.name,f.email,f.phone,f.company,f.message,f.extra,f.created_at FROM feedback f JOIN sites s ON s.id=f.site_id WHERE f.id=? AND f.site_id=? AND f.is_test=0 AND s.active=1").bind(id).bind(claim.site_id).fetch_optional(&mut *tx).await?;
    let row = row.ok_or_else(|| AppError::conflict("訊息或網站已停用"))?;
    let notice = Notice {
        site_name: row.site_name,
        feedback_id: row.id,
        name: row.name,
        email: row.email,
        phone: row.phone,
        company: row.company,
        message: row.message,
        extra: serde_json::from_str(&row.extra)?,
        created_at: row.created_at,
        link: state
            .config
            .public_url
            .as_ref()
            .map(|base| format!("{base}/inbox/{id}")),
    };
    let (channel, target) = if claim.kind == "notify_email" {
        let target = payload["target"]
            .as_str()
            .ok_or_else(|| AppError::bad("通知收件者格式不符"))?
            .to_owned();
        let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sites s JOIN site_members m ON m.site_id=s.id JOIN users u ON u.id=m.user_id WHERE s.id=? AND s.notify_email=1 AND u.disabled=0 AND u.email=?)").bind(row.site_id).bind(&target).fetch_one(&mut *tx).await?;
        if !allowed {
            return Err(AppError::conflict("通知收件者已變更"));
        }
        ("email", target)
    } else {
        let target: String = sqlx::query_scalar("SELECT webhook_url FROM sites WHERE id=?")
            .bind(row.site_id)
            .fetch_one(&mut *tx)
            .await?;
        if payload["target_hash"] != crate::auth::sha256_hex(&target) {
            return Err(AppError::conflict("通知網址已變更"));
        }
        ("webhook", target)
    };
    tx.commit().await?;
    let (ok, detail) = if channel == "email" {
        match &state.mailer {
            Some(m) => match bounded(m.send(&target, &notice)).await {
                Ok(()) => (true, String::new()),
                Err(_) => (false, "SMTP 結果不明；不會自動重寄".to_owned()),
            },
            None => (false, "尚未設定 SMTP".to_owned()),
        }
    } else {
        match bounded(send_webhook(&state.http, &target, &notice)).await {
            Ok(()) => (true, String::new()),
            Err(e) => (false, format!("{e:#}")),
        }
    };
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    crate::jobs::current(&mut tx, claim).await?;
    sqlx::query("INSERT INTO deliveries(feedback_id,channel,target,ok,detail,created_at) VALUES(?,?,?,?,?,?)").bind(id).bind(channel).bind(if channel=="webhook"{mask_url(&target)}else{target}).bind(ok).bind(detail).bind(db::now()).execute(&mut *tx).await?;
    sqlx::query("UPDATE workflow_jobs SET state=?,error_code=?,token=NULL,lease_until=NULL WHERE id=? AND token=?").bind(if ok{"succeeded"}else if channel=="email"&&state.mailer.is_some(){"delivery_unknown"}else{"failed"}).bind((!ok).then_some("notification_failed")).bind(claim.id).bind(&claim.token).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub enum ReplyOutcome {
    Accepted,
    RejectedTransient,
    RejectedPermanent,
    Unknown,
    Invalid,
}
impl Mailer {
    /// Only a definitive negative SMTP reply is retryable. Any I/O/TLS/timeout ambiguity is quarantined.
    pub async fn send_reply(&self, reply: &crate::ai::replies::Outbound) -> ReplyOutcome {
        let Ok(to) = reply.recipient.parse::<Mailbox>() else {
            return ReplyOutcome::Invalid;
        };
        let message = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(&reply.subject)
            .message_id(Some(reply.message_id.clone()))
            .raw_header(lettre::message::header::HeaderValue::new(
                lettre::message::header::HeaderName::new_from_ascii_str("Auto-Submitted"),
                "auto-replied".into(),
            ))
            .multipart(
                MultiPart::alternative()
                    .singlepart(
                        SinglePart::builder()
                            .header(ContentType::TEXT_PLAIN)
                            .body(reply.body.clone()),
                    )
                    .singlepart(SinglePart::builder().header(ContentType::TEXT_HTML).body(
                        format!(
                            "<div style=\"white-space:pre-wrap\">{}</div>",
                            escape_html(&reply.body)
                        ),
                    )),
            );
        let Ok(message) = message else {
            return ReplyOutcome::Invalid;
        };
        match tokio::time::timeout(DEADLINE, self.transport.send(message)).await {
            Ok(Ok(_)) => ReplyOutcome::Accepted,
            Ok(Err(e)) if e.is_permanent() => ReplyOutcome::RejectedPermanent,
            Ok(Err(e)) if e.is_transient() => ReplyOutcome::RejectedTransient,
            _ => ReplyOutcome::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice() -> Notice {
        Notice {
            site_name: "官方網站".into(),
            feedback_id: 7,
            name: "王小明".into(),
            email: "ming@example.com".into(),
            phone: String::new(),
            company: "範例科技".into(),
            message: "想了解 <b>企業方案</b>\n謝謝".into(),
            extra: vec![Extra {
                name: "預算".into(),
                value: "10 萬 & 以上".into(),
            }],
            created_at: "2026-01-05T08:00:00Z".into(),
            link: Some("https://inbox.example.com/inbox/7".into()),
        }
    }

    #[test]
    fn subject_is_one_line() {
        assert_eq!(subject(&notice()), "[官方網站] 王小明（範例科技） 的新訊息");
        let mut n = notice();
        n.name = "x\r\nBcc: someone@example.com".into();
        n.company.clear();
        assert!(!subject(&n).contains('\n') && !subject(&n).contains('\r'));
        n.name.clear();
        assert_eq!(subject(&n), "[官方網站] 訪客 的新訊息");
    }

    #[test]
    fn bodies_skip_empty_fields_and_escape_html() {
        let text = text_body(&notice());
        assert!(text.contains("姓名：王小明") && text.contains("預算：10 萬 & 以上"));
        assert!(!text.contains("電話"));
        assert!(text.contains("https://inbox.example.com/inbox/7"));

        let html = html_body(&notice());
        assert!(html.contains("想了解 &lt;b&gt;企業方案&lt;/b&gt;"));
        assert!(html.contains("10 萬 &amp; 以上"));
        assert!(!html.contains("<b>"));
    }

    #[test]
    fn webhook_format_follows_the_url() {
        let n = notice();
        let discord = webhook_payload("https://discord.com/api/webhooks/1/abc", &n);
        assert_eq!(discord["embeds"][0]["title"], "王小明（範例科技） 的新訊息");
        assert_eq!(discord["allowed_mentions"]["parse"], json!([]));

        let slack = webhook_payload("https://hooks.slack.com/services/T/B/x", &n);
        assert!(slack["text"]
            .as_str()
            .unwrap()
            .contains("預算：10 萬 & 以上"));

        let generic = webhook_payload("https://example.com/hook", &n);
        assert_eq!(generic["event"], "feedback.created");
        assert_eq!(generic["feedback"]["extra"][0]["name"], "預算");

        // 長得像但不是 Discord 的網址用一般格式
        let fake = webhook_payload("https://discord.com.evil.example/api/webhooks/1/abc", &n);
        assert_eq!(fake["event"], "feedback.created");
    }

    #[test]
    fn webhook_urls_are_masked() {
        assert_eq!(
            mask_url("https://discord.com/api/webhooks/1/secret-token"),
            "https://discord.com/…"
        );
        assert_eq!(mask_url("nonsense"), "…");
    }
}
