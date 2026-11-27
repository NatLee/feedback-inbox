//! Immutable approved text revisions. Editing retires the previous revision immediately.
use crate::{
    db,
    error::{AppError, AppResult},
    state::AppState,
    workflow_audit,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Knowledge,
    Template,
}
impl Kind {
    fn table(self) -> &'static str {
        match self {
            Self::Knowledge => "ai_knowledge",
            Self::Template => "ai_templates",
        }
    }
}
#[derive(Deserialize)]
pub struct Body {
    pub expected_revision: Option<i64>,
    pub title: Option<String>,
    pub name: Option<String>,
    pub subject: Option<String>,
    pub body: String,
    #[serde(default)]
    pub knowledge_ids: Vec<i64>,
}
fn dto(row: sqlx::sqlite::SqliteRow, kind: Kind) -> Value {
    let mut value = json!({"id":row.get::<i64,_>("id"),"revision":row.get::<i64,_>("revision"),"body":row.get::<String,_>("body"),"state":row.get::<String,_>("state"),"created_at":row.get::<String,_>("created_at")});
    if kind == Kind::Knowledge {
        value["title"] = json!(row.get::<String, _>("title"))
    } else {
        value["name"] = json!(row.get::<String, _>("name"));
        value["subject"] = json!(row.get::<String, _>("subject"));
        value["knowledge_ids"] =
            serde_json::from_str(row.get::<&str, _>("knowledge_ids_json")).unwrap_or(json!([]));
    }
    value
}
pub async fn list(state: &AppState, site: i64, kind: Kind) -> AppResult<Value> {
    let table = kind.table();
    let rows=sqlx::query(&format!("SELECT * FROM {table} k WHERE site_id=? AND id=(SELECT MAX(id) FROM {table} n WHERE n.entry_id=k.entry_id) ORDER BY id DESC")).bind(site).fetch_all(&state.db).await?;
    Ok(json!({"items":rows.into_iter().map(|r|dto(r,kind)).collect::<Vec<_>>()}))
}
fn bounded(value: &str, min: usize, max: usize) -> bool {
    let n = value.chars().count();
    (min..=max).contains(&n) && !value.contains('\0')
}
fn template_valid(subject: &str, body: &str, ids: &[i64]) -> AppResult<()> {
    for value in [subject, body] {
        let cleaned = value
            .replace("{site_name}", "")
            .replace("{reference}", "")
            .replace("{category}", "");
        if cleaned.contains(['{', '}']) {
            return Err(AppError::bad(
                "範本只支援 site_name、reference、category 預留欄位",
            ));
        }
    }
    if ids.is_empty()
        && (subject != "[{site_name}] 已收到您的訊息"
            || body != "我們已收到您的訊息（編號 {reference}），工作人員會盡快回覆。")
    {
        return Err(AppError::bad(
            "無知識引用時只能使用標準收件確認範本；其他內容需引用核准知識",
        ));
    }
    Ok(())
}
pub async fn save(
    state: &AppState,
    site: i64,
    actor: i64,
    id: Option<i64>,
    kind: Kind,
    b: Body,
) -> AppResult<Value> {
    let label = if kind == Kind::Knowledge {
        b.title.as_deref()
    } else {
        b.name.as_deref()
    }
    .unwrap_or("")
    .trim();
    if !bounded(label, 1, 120) || !bounded(&b.body, 1, 10000) {
        return Err(AppError::bad("標題最多 120 字，內容需為 1–10000 字"));
    }
    let subject = b.subject.as_deref().unwrap_or("");
    if kind == Kind::Template {
        if !bounded(subject, 1, 200) || subject.chars().any(char::is_control) {
            return Err(AppError::bad("範本主旨需為 1–200 字且不能換行"));
        }
        template_valid(subject, &b.body, &b.knowledge_ids)?;
    }
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    super::authority(&mut tx, actor, Some(site), true).await?;
    let table = kind.table();
    let (entry, revision) = if let Some(id) = id {
        let row=sqlx::query(&format!("SELECT * FROM {table} k WHERE id=? AND site_id=? AND id=(SELECT MAX(id) FROM {table} n WHERE n.entry_id=k.entry_id)")).bind(id).bind(site).fetch_optional(&mut *tx).await?.ok_or_else(||AppError::conflict("資料已更新，請重新載入"))?;
        let revision: i64 = row.get("revision");
        if b.expected_revision != Some(revision) {
            return Err(AppError::conflict("資料版本已更新"));
        }
        let entry: i64 = row.get("entry_id");
        sqlx::query(&format!("UPDATE {table} SET state='retired' WHERE id=?"))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        (Some(entry), revision + 1)
    } else {
        let count: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(DISTINCT entry_id) FROM {table} WHERE site_id=? AND state!='retired'"
        ))
        .bind(site)
        .fetch_one(&mut *tx)
        .await?;
        if count >= 50 {
            return Err(AppError::bad("每個網站最多 50 筆有效資料"));
        }
        (None, 1)
    };
    if kind == Kind::Template {
        super::policy::knowledge_valid(&mut tx, site, &b.knowledge_ids).await?;
    }
    let id = if kind == Kind::Knowledge {
        sqlx::query("INSERT INTO ai_knowledge(site_id,entry_id,revision,title,body,created_at) VALUES(?,?,?,?,?,?)").bind(site).bind(entry).bind(revision).bind(label).bind(&b.body).bind(db::now()).execute(&mut *tx).await?.last_insert_rowid()
    } else {
        sqlx::query("INSERT INTO ai_templates(site_id,entry_id,revision,name,subject,body,knowledge_ids_json,created_at) VALUES(?,?,?,?,?,?,?,?)").bind(site).bind(entry).bind(revision).bind(label).bind(subject).bind(&b.body).bind(serde_json::to_string(&b.knowledge_ids)?).bind(db::now()).execute(&mut *tx).await?.last_insert_rowid()
    };
    if entry.is_none() {
        sqlx::query(&format!("UPDATE {table} SET entry_id=id WHERE id=?"))
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    super::policy::touch(&mut tx, site).await?;
    workflow_audit::record(
        &mut tx,
        Some(site),
        Some(actor),
        table,
        id,
        "revision_created",
        json!({"revision":revision}),
    )
    .await?;
    let result = dto(
        sqlx::query(&format!("SELECT * FROM {table} WHERE id=?"))
            .bind(id)
            .fetch_one(&mut *tx)
            .await?,
        kind,
    );
    tx.commit().await?;
    Ok(result)
}
pub async fn transition(
    state: &AppState,
    site: i64,
    actor: i64,
    id: i64,
    kind: Kind,
    revision: i64,
    action: &str,
) -> AppResult<Value> {
    let table = kind.table();
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    super::authority(&mut tx, actor, Some(site), true).await?;
    let row=sqlx::query(&format!("SELECT * FROM {table} k WHERE id=? AND site_id=? AND revision=? AND id=(SELECT MAX(id) FROM {table} n WHERE n.entry_id=k.entry_id)")).bind(id).bind(site).bind(revision).fetch_optional(&mut *tx).await?.ok_or_else(||AppError::conflict("資料已更新或不存在"))?;
    if action == "approve" {
        if row.get::<String, _>("state") == "retired" {
            return Err(AppError::conflict("已停用的版本不可重新核准，請建立新版本"));
        }
        if kind == Kind::Template {
            let ids: Vec<i64> = serde_json::from_str(row.get::<&str, _>("knowledge_ids_json"))?;
            super::policy::knowledge_valid(&mut tx, site, &ids).await?;
        }
    }
    let target = match action {
        "approve" => "approved",
        "retire" => "retired",
        _ => return Err(AppError::not_found("未知動作")),
    };
    sqlx::query(&format!(
        "UPDATE {table} SET state=?,approved_by=? WHERE id=?"
    ))
    .bind(target)
    .bind(actor)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    super::policy::touch(&mut tx, site).await?;
    workflow_audit::record(
        &mut tx,
        Some(site),
        Some(actor),
        table,
        id,
        action,
        json!({"revision":revision}),
    )
    .await?;
    let result = dto(
        sqlx::query(&format!("SELECT * FROM {table} WHERE id=?"))
            .bind(id)
            .fetch_one(&mut *tx)
            .await?,
        kind,
    );
    tx.commit().await?;
    Ok(result)
}
