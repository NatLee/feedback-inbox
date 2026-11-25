//! 網站（表單）的設定。管理員看得到全部，其他人只看得到自己負責的。

use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    auth::{self, Admin, CurrentUser},
    config::origin_of,
    db,
    error::{AppError, AppResult},
    intake::{Extra, FIELDS},
    notify::{self, Notice},
    state::AppState,
};

#[derive(sqlx::FromRow)]
struct SiteRow {
    id: i64,
    name: String,
    url: String,
    key: String,
    allowed_origins: String,
    required_fields: String,
    redirect_url: String,
    webhook_url: String,
    notify_email: bool,
    active: bool,
    created_at: String,
    total: i64,
    unhandled: i64,
}

#[derive(Serialize, Clone)]
pub struct Member {
    id: i64,
    username: String,
    display_name: String,
    /// 有沒有填信箱（沒填就收不到通知信）
    has_email: bool,
}

#[derive(Serialize)]
pub struct Site {
    id: i64,
    name: String,
    url: String,
    key: String,
    allowed_origins: Vec<String>,
    required_fields: Vec<String>,
    redirect_url: String,
    /// 只有管理員拿得到；其他人是空字串
    webhook_url: String,
    has_webhook: bool,
    notify_email: bool,
    active: bool,
    created_at: String,
    total: i64,
    /// 還沒處理完的（新的和處理中的）
    unhandled: i64,
    members: Vec<Member>,
}

const SITE_SELECT: &str =
    "SELECT s.id, s.name, s.url, s.key, s.allowed_origins, s.required_fields, s.redirect_url,
        s.webhook_url, s.notify_email, s.active, s.created_at,
        (SELECT COUNT(*) FROM feedback f WHERE f.site_id = s.id) AS total,
        (SELECT COUNT(*) FROM feedback f
          WHERE f.site_id = s.id AND f.status IN ('new', 'in_progress')) AS unhandled
     FROM sites s";

pub fn parse_list(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

async fn members_by_site(state: &AppState) -> AppResult<HashMap<i64, Vec<Member>>> {
    let rows: Vec<(i64, i64, String, String, String)> = sqlx::query_as(
        "SELECT m.site_id, u.id, u.username, u.display_name, u.email
         FROM site_members m JOIN users u ON u.id = m.user_id
         WHERE u.disabled = 0 ORDER BY u.display_name",
    )
    .fetch_all(&state.db)
    .await?;
    let mut map: HashMap<i64, Vec<Member>> = HashMap::new();
    for (site_id, id, username, display_name, email) in rows {
        map.entry(site_id).or_default().push(Member {
            id,
            username,
            display_name,
            has_email: !email.is_empty(),
        });
    }
    Ok(map)
}

fn present(row: SiteRow, members: Vec<Member>, admin: bool) -> Site {
    Site {
        id: row.id,
        name: row.name,
        url: row.url,
        key: row.key,
        allowed_origins: parse_list(&row.allowed_origins),
        required_fields: parse_list(&row.required_fields),
        redirect_url: row.redirect_url,
        has_webhook: !row.webhook_url.is_empty(),
        webhook_url: if admin {
            row.webhook_url
        } else {
            String::new()
        },
        notify_email: row.notify_email,
        active: row.active,
        created_at: row.created_at,
        total: row.total,
        unhandled: row.unhandled,
        members,
    }
}

/// 這個人看不看得到這個網站的訊息。
pub async fn ensure_visible(state: &AppState, user: &CurrentUser, site_id: i64) -> AppResult<()> {
    if user.is_admin() {
        return Ok(());
    }
    let member: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM site_members WHERE site_id = ? AND user_id = ?")
            .bind(site_id)
            .bind(user.id)
            .fetch_optional(&state.db)
            .await?;
    match member {
        Some(_) => Ok(()),
        // 不說「沒有權限」，免得別人用編號去猜有哪些網站
        None => Err(AppError::not_found("找不到這個網站")),
    }
}

pub async fn list(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<Site>>> {
    let rows: Vec<SiteRow> = if user.is_admin() {
        sqlx::query_as(&format!("{SITE_SELECT} ORDER BY s.name"))
            .fetch_all(&state.db)
            .await?
    } else {
        sqlx::query_as(&format!(
            "{SITE_SELECT} WHERE s.id IN (SELECT site_id FROM site_members WHERE user_id = ?)
             ORDER BY s.name"
        ))
        .bind(user.id)
        .fetch_all(&state.db)
        .await?
    };
    let mut members = members_by_site(&state).await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| {
                let m = members.remove(&row.id).unwrap_or_default();
                present(row, m, user.is_admin())
            })
            .collect(),
    ))
}

async fn one(state: &AppState, user: &CurrentUser, id: i64) -> AppResult<Site> {
    let row: SiteRow = sqlx::query_as(&format!("{SITE_SELECT} WHERE s.id = ?"))
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這個網站"))?;
    ensure_visible(state, user, id).await?;
    let members = members_by_site(state)
        .await?
        .remove(&id)
        .unwrap_or_default();
    Ok(present(row, members, user.is_admin()))
}

pub async fn detail(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Site>> {
    Ok(Json(one(&state, &user, id).await?))
}

#[derive(Deserialize, Default)]
pub struct SiteBody {
    name: Option<String>,
    url: Option<String>,
    allowed_origins: Option<Vec<String>>,
    required_fields: Option<Vec<String>>,
    redirect_url: Option<String>,
    webhook_url: Option<String>,
    notify_email: Option<bool>,
    active: Option<bool>,
    member_ids: Option<Vec<i64>>,
}

fn clean_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(AppError::bad("網站名稱要 1 到 60 個字元"));
    }
    Ok(name.to_owned())
}

/// 可以留白；有填的話要是 http(s) 網址。
fn clean_url(url: &str, label: &str) -> AppResult<String> {
    let url = url.trim();
    if url.is_empty() {
        return Ok(String::new());
    }
    if url.len() > 500 || origin_of(url).is_none() {
        return Err(AppError::bad(format!(
            "{label}要是 http:// 或 https:// 開頭的網址"
        )));
    }
    Ok(url.to_owned())
}

/// `https://www.example.com/contact` 也收，只留 origin。
fn clean_origins(origins: &[String]) -> AppResult<String> {
    let mut out: Vec<String> = Vec::new();
    for origin in origins.iter().filter(|o| !o.trim().is_empty()) {
        let origin = origin_of(origin).ok_or_else(|| {
            AppError::bad(format!(
                "「{}」不是合法的來源，要像 https://www.example.com",
                origin.trim()
            ))
        })?;
        if !out.contains(&origin) {
            out.push(origin);
        }
    }
    if out.len() > 20 {
        return Err(AppError::bad("允許的來源最多 20 個"));
    }
    Ok(serde_json::to_string(&out)?)
}

fn clean_required(fields: &[String]) -> AppResult<String> {
    if let Some(unknown) = fields.iter().find(|f| !FIELDS.contains(&f.as_str())) {
        return Err(AppError::bad(format!("沒有「{unknown}」這個欄位")));
    }
    // 照固定的順序存
    let ordered: Vec<&str> = FIELDS
        .into_iter()
        .filter(|f| fields.iter().any(|x| x == f))
        .collect();
    Ok(serde_json::to_string(&ordered)?)
}

async fn name_taken(state: &AppState, name: &str, except: i64) -> AppResult<bool> {
    let taken: Option<i64> = sqlx::query_scalar("SELECT id FROM sites WHERE name = ? AND id != ?")
        .bind(name)
        .bind(except)
        .fetch_optional(&state.db)
        .await?;
    Ok(taken.is_some())
}

async fn set_members(state: &AppState, site_id: i64, member_ids: &[i64]) -> AppResult<()> {
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM site_members WHERE site_id = ?")
        .bind(site_id)
        .execute(&mut *tx)
        .await?;
    let mut ids = member_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    for user_id in ids {
        let added = sqlx::query(
            "INSERT INTO site_members (site_id, user_id) SELECT ?, id FROM users WHERE id = ?",
        )
        .bind(site_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
        if added.rows_affected() == 0 {
            return Err(AppError::bad(format!("找不到帳號 {user_id}")));
        }
    }
    tx.commit().await?;
    Ok(())
}

pub async fn create(
    State(state): State<AppState>,
    Admin(admin): Admin,
    Json(body): Json<SiteBody>,
) -> AppResult<Json<Site>> {
    let name = clean_name(body.name.as_deref().unwrap_or_default())?;
    if name_taken(&state, &name, 0).await? {
        return Err(AppError::conflict("已經有同名的網站"));
    }
    let url = clean_url(body.url.as_deref().unwrap_or_default(), "網址")?;
    let origins = clean_origins(body.allowed_origins.as_deref().unwrap_or_default())?;
    let required = match &body.required_fields {
        Some(fields) => clean_required(fields)?,
        None => r#"["name","email","message"]"#.to_owned(),
    };
    let redirect_url = clean_url(
        body.redirect_url.as_deref().unwrap_or_default(),
        "送出後轉去的網址",
    )?;
    let webhook_url = clean_url(
        body.webhook_url.as_deref().unwrap_or_default(),
        "Webhook 網址",
    )?;
    let id = sqlx::query(
        "INSERT INTO sites (name, url, key, allowed_origins, required_fields, redirect_url,
            webhook_url, notify_email, active, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&name)
    .bind(url)
    .bind(auth::random_hex(8))
    .bind(origins)
    .bind(required)
    .bind(redirect_url)
    .bind(webhook_url)
    .bind(body.notify_email.unwrap_or(true))
    .bind(body.active.unwrap_or(true))
    .bind(db::now())
    .execute(&state.db)
    .await?
    .last_insert_rowid();
    // 沒指定負責人時，先放建立的人
    let member_ids = body.member_ids.unwrap_or_else(|| vec![admin.id]);
    set_members(&state, id, &member_ids).await?;
    Ok(Json(one(&state, &admin, id).await?))
}

pub async fn update(
    State(state): State<AppState>,
    Admin(admin): Admin,
    Path(id): Path<i64>,
    Json(body): Json<SiteBody>,
) -> AppResult<Json<Site>> {
    let current = one(&state, &admin, id).await?;
    let name = match &body.name {
        Some(name) => clean_name(name)?,
        None => current.name.clone(),
    };
    if name_taken(&state, &name, id).await? {
        return Err(AppError::conflict("已經有同名的網站"));
    }
    let url = match &body.url {
        Some(url) => clean_url(url, "網址")?,
        None => current.url.clone(),
    };
    let origins = clean_origins(
        body.allowed_origins
            .as_deref()
            .unwrap_or(&current.allowed_origins),
    )?;
    let required = clean_required(
        body.required_fields
            .as_deref()
            .unwrap_or(&current.required_fields),
    )?;
    let redirect_url = match &body.redirect_url {
        Some(url) => clean_url(url, "送出後轉去的網址")?,
        None => current.redirect_url.clone(),
    };
    let webhook_url = match &body.webhook_url {
        Some(url) => clean_url(url, "Webhook 網址")?,
        None => current.webhook_url.clone(),
    };
    sqlx::query(
        "UPDATE sites SET name = ?, url = ?, allowed_origins = ?, required_fields = ?,
            redirect_url = ?, webhook_url = ?, notify_email = ?, active = ?
         WHERE id = ?",
    )
    .bind(name)
    .bind(url)
    .bind(origins)
    .bind(required)
    .bind(redirect_url)
    .bind(webhook_url)
    .bind(body.notify_email.unwrap_or(current.notify_email))
    .bind(body.active.unwrap_or(current.active))
    .bind(id)
    .execute(&state.db)
    .await?;
    if let Some(member_ids) = &body.member_ids {
        set_members(&state, id, member_ids).await?;
    }
    Ok(Json(one(&state, &admin, id).await?))
}

/// 刪掉網站，它收到的訊息也一起刪。
pub async fn remove(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let done = sqlx::query("DELETE FROM sites WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::not_found("找不到這個網站"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// 換一個新的表單網址；舊的馬上失效（網址外流、被拿去灌垃圾訊息時用）。
pub async fn rotate_key(
    State(state): State<AppState>,
    Admin(admin): Admin,
    Path(id): Path<i64>,
) -> AppResult<Json<Site>> {
    let done = sqlx::query("UPDATE sites SET key = ? WHERE id = ?")
        .bind(auth::random_hex(8))
        .bind(id)
        .execute(&state.db)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::not_found("找不到這個網站"));
    }
    Ok(Json(one(&state, &admin, id).await?))
}

/// 送一則測試通知，馬上回報每個目標有沒有成功。不會在收件匣裡留下訊息。
pub async fn test_notification(
    State(state): State<AppState>,
    Admin(admin): Admin,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let site = one(&state, &admin, id).await?;
    let notice = Notice {
        site_name: site.name,
        feedback_id: 0,
        name: "測試通知".into(),
        email: String::new(),
        phone: String::new(),
        company: String::new(),
        message: format!(
            "這是 {} 從「網站設定」送出的測試通知。收到這則訊息，表示通知設定沒有問題。",
            admin.display_name
        ),
        extra: vec![Extra {
            name: "送出的人".into(),
            value: admin.username,
        }],
        created_at: db::now(),
        link: state.config.public_url.clone(),
    };
    let targets = notify::targets(&state, id).await?;
    let outcomes = notify::send(&state, &targets, &notice).await;
    Ok(Json(json!({
        "outcomes": outcomes,
        "smtp_configured": state.mailer.is_some(),
        // 設定了寄信，但負責人都沒填信箱
        "no_recipients": site.notify_email && targets.emails.is_empty(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn origins_are_normalised() {
        let cleaned = clean_origins(&strings(&[
            "https://WWW.example.com/contact",
            "https://www.example.com",
            " ",
            "http://localhost:5173",
        ]))
        .unwrap();
        assert_eq!(
            cleaned,
            r#"["https://www.example.com","http://localhost:5173"]"#
        );
        assert!(clean_origins(&strings(&["www.example.com"])).is_err());
    }

    #[test]
    fn required_fields_are_checked_and_ordered() {
        assert_eq!(
            clean_required(&strings(&["message", "name"])).unwrap(),
            r#"["name","message"]"#
        );
        assert!(clean_required(&strings(&["address"])).is_err());
    }

    #[test]
    fn urls() {
        assert_eq!(clean_url("  ", "網址").unwrap(), "");
        assert!(clean_url("https://example.com/thanks", "網址").is_ok());
        assert!(clean_url("javascript:alert(1)", "網址").is_err());
        assert!(clean_url("/thanks", "網址").is_err());
    }
}
