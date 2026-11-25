//! 收件匣：列出、搜尋、改狀態、寫備註、匯出。

use axum::{
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{QueryBuilder, Sqlite};

use crate::{
    auth::{Admin, CurrentUser},
    db,
    error::{AppError, AppResult},
    intake::{truncate, Extra},
    state::AppState,
};

pub const STATUSES: [&str; 4] = ["new", "in_progress", "done", "spam"];

const DEFAULT_PER_PAGE: u32 = 25;
const MAX_PER_PAGE: u32 = 100;
const MAX_EXPORT_ROWS: i64 = 50_000;
const MAX_BULK: usize = 200;
const MAX_NOTE_CHARS: usize = 2000;

pub fn status_label(status: &str) -> &'static str {
    match status {
        "new" => "未處理",
        "in_progress" => "處理中",
        "done" => "已完成",
        _ => "垃圾訊息",
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    site_id: i64,
    site_name: String,
    name: String,
    email: String,
    phone: String,
    company: String,
    message: String,
    extra: String,
    status: String,
    ip: String,
    user_agent: String,
    page_url: String,
    created_at: String,
    updated_at: String,
    note_count: i64,
}

const SELECT: &str = "SELECT f.id, f.site_id, s.name AS site_name, f.name, f.email, f.phone,
        f.company, f.message, f.extra, f.status, f.ip, f.user_agent, f.page_url,
        f.created_at, f.updated_at,
        (SELECT COUNT(*) FROM notes n WHERE n.feedback_id = f.id AND n.kind = 'note') AS note_count
     FROM feedback f JOIN sites s ON s.id = f.site_id";

/// 列表裡的一列。
#[derive(Serialize)]
pub struct Item {
    id: i64,
    site_id: i64,
    site_name: String,
    name: String,
    email: String,
    company: String,
    /// 訊息的開頭，壓成一行
    preview: String,
    status: String,
    created_at: String,
    note_count: i64,
}

fn item(row: Row) -> Item {
    let one_line = row.message.split_whitespace().collect::<Vec<_>>().join(" ");
    Item {
        id: row.id,
        site_id: row.site_id,
        site_name: row.site_name,
        name: row.name,
        email: row.email,
        company: row.company,
        preview: truncate(&one_line, 120),
        status: row.status,
        created_at: row.created_at,
        note_count: row.note_count,
    }
}

#[derive(Deserialize, Default)]
pub struct Filter {
    site_id: Option<i64>,
    /// `new` / `in_progress` / `done` / `spam`、`open`（還沒處理完的）或 `all`。
    /// 沒給的話是垃圾訊息以外的全部。
    status: Option<String>,
    q: Option<String>,
    from: Option<String>,
    to: Option<String>,
    page: Option<u32>,
    per_page: Option<u32>,
    /// 匯出時把時間換成當地時間用：比 UTC 快幾分鐘（台北是 480）
    tz_offset: Option<i32>,
}

fn parse_time(text: &str, label: &str) -> AppResult<String> {
    DateTime::parse_from_rfc3339(text.trim())
        .map(|t| db::format_time(t.with_timezone(&Utc)))
        .map_err(|_| {
            AppError::bad(format!(
                "{label}要是 RFC 3339 的時間，例如 2026-01-05T00:00:00+08:00"
            ))
        })
}

/// `LIKE` 的萬用字元當成一般的字。
fn like_pattern(q: &str) -> String {
    let mut out = String::from("%");
    for c in q.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

/// 只看得到自己負責的網站的訊息（管理員看全部）。
pub fn push_visible(qb: &mut QueryBuilder<'_, Sqlite>, user: &CurrentUser) {
    if !user.is_admin() {
        qb.push(" AND f.site_id IN (SELECT site_id FROM site_members WHERE user_id = ")
            .push_bind(user.id)
            .push(")");
    }
}

fn push_filters(
    qb: &mut QueryBuilder<'_, Sqlite>,
    user: &CurrentUser,
    filter: &Filter,
) -> AppResult<()> {
    qb.push(" WHERE 1 = 1");
    push_visible(qb, user);
    if let Some(site_id) = filter.site_id {
        qb.push(" AND f.site_id = ").push_bind(site_id);
    }
    match filter.status.as_deref().map(str::trim) {
        None | Some("") => {
            qb.push(" AND f.status != 'spam'");
        }
        Some("all") => {}
        Some("open") => {
            qb.push(" AND f.status IN ('new', 'in_progress')");
        }
        Some(status) if STATUSES.contains(&status) => {
            qb.push(" AND f.status = ").push_bind(status.to_owned());
        }
        Some(other) => return Err(AppError::bad(format!("沒有「{other}」這個狀態"))),
    }
    if let Some(q) = filter.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        let pattern = like_pattern(q);
        qb.push(" AND (");
        for (i, column) in ["f.name", "f.email", "f.phone", "f.company", "f.message"]
            .into_iter()
            .enumerate()
        {
            if i > 0 {
                qb.push(" OR ");
            }
            qb.push(column)
                .push(" LIKE ")
                .push_bind(pattern.clone())
                .push(" ESCAPE '\\'");
        }
        qb.push(")");
    }
    if let Some(from) = filter.from.as_deref().filter(|t| !t.trim().is_empty()) {
        qb.push(" AND f.created_at >= ")
            .push_bind(parse_time(from, "from")?);
    }
    if let Some(to) = filter.to.as_deref().filter(|t| !t.trim().is_empty()) {
        qb.push(" AND f.created_at < ")
            .push_bind(parse_time(to, "to")?);
    }
    Ok(())
}

pub async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(filter): Query<Filter>,
) -> AppResult<Json<Value>> {
    let per_page = filter
        .per_page
        .unwrap_or(DEFAULT_PER_PAGE)
        .clamp(1, MAX_PER_PAGE);
    let page = filter.page.unwrap_or(1).max(1);

    let mut count = QueryBuilder::new("SELECT COUNT(*) FROM feedback f");
    push_filters(&mut count, &user, &filter)?;
    let total: i64 = count.build_query_scalar().fetch_one(&state.db).await?;

    let mut qb = QueryBuilder::new(SELECT);
    push_filters(&mut qb, &user, &filter)?;
    qb.push(" ORDER BY f.created_at DESC, f.id DESC LIMIT ")
        .push_bind(i64::from(per_page))
        .push(" OFFSET ")
        .push_bind(i64::from(page - 1) * i64::from(per_page));
    let rows: Vec<Row> = qb.build_query_as().fetch_all(&state.db).await?;

    Ok(Json(json!({
        "items": rows.into_iter().map(item).collect::<Vec<_>>(),
        "total": total,
        "page": page,
        "per_page": per_page,
    })))
}

/// 最新的幾則（首頁用）。
pub async fn recent(state: &AppState, user: &CurrentUser, limit: i64) -> AppResult<Vec<Item>> {
    let mut qb = QueryBuilder::new(SELECT);
    qb.push(" WHERE f.status != 'spam'");
    push_visible(&mut qb, user);
    qb.push(" ORDER BY f.created_at DESC, f.id DESC LIMIT ")
        .push_bind(limit);
    let rows: Vec<Row> = qb.build_query_as().fetch_all(&state.db).await?;
    Ok(rows.into_iter().map(item).collect())
}

/// 讀一則訊息；看不到的當成不存在。
async fn one(state: &AppState, user: &CurrentUser, id: i64) -> AppResult<Row> {
    let mut qb = QueryBuilder::new(SELECT);
    qb.push(" WHERE f.id = ").push_bind(id);
    push_visible(&mut qb, user);
    qb.build_query_as()
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這則訊息"))
}

#[derive(Serialize, sqlx::FromRow)]
struct Note {
    id: i64,
    user_id: i64,
    author: String,
    /// `note` 是備註；`status` 是狀態變更，`body` 是改成的狀態
    kind: String,
    body: String,
    created_at: String,
}

#[derive(Serialize, sqlx::FromRow)]
struct Delivery {
    channel: String,
    target: String,
    ok: bool,
    detail: String,
    created_at: String,
}

async fn present(state: &AppState, row: Row) -> AppResult<Value> {
    let notes: Vec<Note> = sqlx::query_as(
        "SELECT n.id, n.user_id, u.display_name AS author, n.kind, n.body, n.created_at
         FROM notes n JOIN users u ON u.id = n.user_id
         WHERE n.feedback_id = ? ORDER BY n.created_at, n.id",
    )
    .bind(row.id)
    .fetch_all(&state.db)
    .await?;
    let deliveries: Vec<Delivery> = sqlx::query_as(
        "SELECT channel, target, ok, detail, created_at FROM deliveries
         WHERE feedback_id = ? ORDER BY id",
    )
    .bind(row.id)
    .fetch_all(&state.db)
    .await?;
    let extra: Vec<Extra> = serde_json::from_str(&row.extra).unwrap_or_default();
    Ok(json!({
        "id": row.id,
        "site_id": row.site_id,
        "site_name": row.site_name,
        "name": row.name,
        "email": row.email,
        "phone": row.phone,
        "company": row.company,
        "message": row.message,
        "extra": extra,
        "status": row.status,
        "ip": row.ip,
        "user_agent": row.user_agent,
        "page_url": row.page_url,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
        "notes": notes,
        "deliveries": deliveries,
    }))
}

pub async fn detail(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let row = one(&state, &user, id).await?;
    Ok(Json(present(&state, row).await?))
}

fn check_status(status: &str) -> AppResult<()> {
    if STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(AppError::bad("狀態只能是 new、in_progress、done 或 spam"))
    }
}

/// 改狀態，並在處理紀錄裡留一筆。狀態沒變的話什麼都不做。
async fn set_status(
    tx: &mut sqlx::SqliteConnection,
    user: &CurrentUser,
    id: i64,
    status: &str,
) -> AppResult<bool> {
    let now = db::now();
    let changed =
        sqlx::query("UPDATE feedback SET status = ?, updated_at = ? WHERE id = ? AND status != ?")
            .bind(status)
            .bind(&now)
            .bind(id)
            .bind(status)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;
    if changed {
        sqlx::query(
            "INSERT INTO notes (feedback_id, user_id, kind, body, created_at)
             VALUES (?, ?, 'status', ?, ?)",
        )
        .bind(id)
        .bind(user.id)
        .bind(status)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }
    Ok(changed)
}

#[derive(Deserialize)]
pub struct StatusBody {
    status: String,
}

pub async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<StatusBody>,
) -> AppResult<Json<Value>> {
    check_status(&body.status)?;
    one(&state, &user, id).await?;
    let mut tx = state.db.begin().await?;
    set_status(&mut tx, &user, id, &body.status).await?;
    tx.commit().await?;
    let row = one(&state, &user, id).await?;
    Ok(Json(present(&state, row).await?))
}

#[derive(Deserialize)]
pub struct BulkBody {
    ids: Vec<i64>,
    status: String,
}

/// 一次改很多則的狀態。看不到的那些會被跳過。
pub async fn bulk(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<BulkBody>,
) -> AppResult<Json<Value>> {
    check_status(&body.status)?;
    if body.ids.is_empty() || body.ids.len() > MAX_BULK {
        return Err(AppError::bad(format!("一次要選 1 到 {MAX_BULK} 則")));
    }
    let mut qb = QueryBuilder::new("SELECT f.id FROM feedback f WHERE f.id IN (");
    let mut ids = qb.separated(", ");
    for id in &body.ids {
        ids.push_bind(*id);
    }
    qb.push(")");
    push_visible(&mut qb, &user);
    let visible: Vec<i64> = qb.build_query_scalar().fetch_all(&state.db).await?;

    let mut tx = state.db.begin().await?;
    let mut updated = 0;
    for id in visible {
        if set_status(&mut tx, &user, id, &body.status).await? {
            updated += 1;
        }
    }
    tx.commit().await?;
    Ok(Json(json!({ "updated": updated })))
}

pub async fn remove(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let done = sqlx::query("DELETE FROM feedback WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::not_found("找不到這則訊息"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct NoteBody {
    body: String,
}

pub async fn add_note(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<NoteBody>,
) -> AppResult<Json<Value>> {
    let text = body.body.trim();
    if text.is_empty() || text.chars().count() > MAX_NOTE_CHARS {
        return Err(AppError::bad(format!("備註要 1 到 {MAX_NOTE_CHARS} 個字")));
    }
    one(&state, &user, id).await?;
    let now = db::now();
    sqlx::query(
        "INSERT INTO notes (feedback_id, user_id, kind, body, created_at)
         VALUES (?, ?, 'note', ?, ?)",
    )
    .bind(id)
    .bind(user.id)
    .bind(text)
    .bind(&now)
    .execute(&state.db)
    .await?;
    sqlx::query("UPDATE feedback SET updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(id)
        .execute(&state.db)
        .await?;
    let row = one(&state, &user, id).await?;
    Ok(Json(present(&state, row).await?))
}

/// 刪掉備註：只有寫的人和管理員可以。狀態變更的紀錄不能刪。
pub async fn remove_note(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let note: Option<(i64, i64, String)> =
        sqlx::query_as("SELECT feedback_id, user_id, kind FROM notes WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let not_found = || AppError::not_found("找不到這則備註");
    let (feedback_id, author_id, kind) = note.ok_or_else(not_found)?;
    one(&state, &user, feedback_id)
        .await
        .map_err(|_| not_found())?;
    if kind != "note" {
        return Err(AppError::bad("狀態變更的紀錄不能刪"));
    }
    if author_id != user.id && !user.is_admin() {
        return Err(AppError::forbidden("只能刪自己寫的備註"));
    }
    sqlx::query("DELETE FROM notes WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(Json(json!({ "ok": true })))
}

/// 試算表會把 `=`、`+`、`-`、`@` 開頭的格子當成公式；前面加一個單引號擋掉。
fn csv_cell(text: &str) -> String {
    if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_owned()
    }
}

fn local_time(utc: &str, tz_offset: i32) -> String {
    match DateTime::parse_from_rfc3339(utc) {
        Ok(t) => (t.naive_utc() + Duration::minutes(i64::from(tz_offset)))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        Err(_) => utc.to_owned(),
    }
}

/// 把目前篩選出來的訊息匯出成 CSV。
pub async fn export(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(filter): Query<Filter>,
) -> AppResult<Response> {
    let tz_offset = filter.tz_offset.unwrap_or(0).clamp(-14 * 60, 14 * 60);
    let mut qb = QueryBuilder::new(SELECT);
    push_filters(&mut qb, &user, &filter)?;
    qb.push(" ORDER BY f.created_at DESC, f.id DESC LIMIT ")
        .push_bind(MAX_EXPORT_ROWS);
    let rows: Vec<Row> = qb.build_query_as().fetch_all(&state.db).await?;

    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record([
        "編號",
        "網站",
        "時間",
        "狀態",
        "姓名",
        "Email",
        "電話",
        "公司",
        "訊息",
        "其他欄位",
    ])?;
    for row in rows {
        let extra: Vec<Extra> = serde_json::from_str(&row.extra).unwrap_or_default();
        let extra = extra
            .iter()
            .map(|e| format!("{}：{}", e.name, e.value))
            .collect::<Vec<_>>()
            .join("\n");
        writer.write_record([
            row.id.to_string(),
            csv_cell(&row.site_name),
            local_time(&row.created_at, tz_offset),
            status_label(&row.status).to_owned(),
            csv_cell(&row.name),
            csv_cell(&row.email),
            csv_cell(&row.phone),
            csv_cell(&row.company),
            csv_cell(&row.message),
            csv_cell(&extra),
        ])?;
    }
    // 開頭加 BOM，Excel 才會用 UTF-8 開
    let mut body = b"\xEF\xBB\xBF".to_vec();
    body.extend(writer.into_inner().map_err(|e| anyhow::anyhow!("{e}"))?);
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"feedback.csv\"",
            ),
        ],
        body,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_wildcards_are_escaped() {
        assert_eq!(like_pattern("50%_off"), "%50\\%\\_off%");
    }

    #[test]
    fn formula_cells_are_defused() {
        assert_eq!(csv_cell("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(csv_cell("+886 900 000 000"), "'+886 900 000 000");
        assert_eq!(csv_cell("你好"), "你好");
    }

    #[test]
    fn times() {
        assert_eq!(
            local_time("2026-01-05T20:30:00Z", 480),
            "2026-01-06 04:30:00"
        );
        assert_eq!(
            parse_time("2026-01-05T00:00:00+08:00", "from").unwrap(),
            "2026-01-04T16:00:00Z"
        );
        assert!(parse_time("2026-01-05", "from").is_err());
    }
}
