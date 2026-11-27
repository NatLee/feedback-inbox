use crate::{
    db,
    error::{AppError, AppResult},
    forms::{self, Sensitivity},
    state::AppState,
    workflow_audit,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub revision: i64,
    pub enabled: bool,
    pub provider_id: Option<i64>,
    pub generate_on_receipt: bool,
    pub field_ids: Vec<String>,
    pub include_personal: bool,
    pub categories: Vec<String>,
    pub tags: Vec<String>,
    pub daily_attempt_limit: i64,
    pub timeout_seconds: u64,
    pub max_output_tokens: u32,
    pub auto_mode: String,
    pub auto_daily_limit: i64,
    pub auto_activated_at: Option<i64>,
    #[serde(skip)]
    pub updated_by: Option<i64>,
    #[serde(skip)]
    pub activation_feedback_id: Option<i64>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            revision: 1,
            enabled: false,
            provider_id: None,
            generate_on_receipt: false,
            field_ids: vec![],
            include_personal: false,
            categories: vec![
                "一般詢問".into(),
                "產品問題".into(),
                "功能建議".into(),
                "其他".into(),
            ],
            tags: vec![],
            daily_attempt_limit: 100,
            timeout_seconds: 60,
            max_output_tokens: 1000,
            auto_mode: "off".into(),
            auto_daily_limit: 20,
            auto_activated_at: None,
            updated_by: None,
            activation_feedback_id: None,
        }
    }
}
pub async fn load(tx: &mut sqlx::SqliteConnection, site: i64) -> AppResult<Settings> {
    type SettingsRow = (i64, String, Option<i64>, Option<i64>, Option<i64>);
    let row:Option<SettingsRow>=sqlx::query_as("SELECT revision,settings_json,updated_by,auto_activated_at,activation_feedback_id FROM site_ai_settings WHERE site_id=?").bind(site).fetch_optional(tx).await?;
    if let Some((revision, raw, actor, activated, highwater)) = row {
        let mut s: Settings = serde_json::from_str(&raw)?;
        s.revision = revision;
        s.updated_by = actor;
        s.auto_activated_at = activated;
        s.activation_feedback_id = highwater;
        Ok(s)
    } else {
        Ok(Settings::default())
    }
}
fn taxonomy(values: &[String], empty: bool) -> bool {
    (empty || !values.is_empty())
        && values.len() <= 30
        && values.iter().all(|v| {
            !v.trim().is_empty() && v.chars().count() <= 60 && !v.chars().any(char::is_control)
        })
        && values
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            == values.len()
}
pub async fn save(
    state: &AppState,
    site: i64,
    actor: i64,
    revision: i64,
    mut body: Settings,
) -> AppResult<Settings> {
    let form = forms::load_version(&state.db, site, None).await?;
    if !(1..=1000).contains(&body.daily_attempt_limit)
        || !(5..=120).contains(&body.timeout_seconds)
        || !(128..=4096).contains(&body.max_output_tokens)
        || !(1..=200).contains(&body.auto_daily_limit)
        || !matches!(body.auto_mode.as_str(), "off" | "auto_template")
        || !taxonomy(&body.categories, false)
        || !taxonomy(&body.tags, true)
    {
        return Err(AppError::bad("AI 分類、用量或時間限制不符"));
    }
    if body.field_ids.len() > 30
        || body
            .field_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != body.field_ids.len()
    {
        return Err(AppError::bad("選取欄位格式不符"));
    }
    for id in &body.field_ids {
        let field = form
            .schema
            .fields
            .iter()
            .find(|f| &f.id == id)
            .ok_or_else(|| AppError::bad("選取欄位不在目前發布表單內"))?;
        if field.sensitivity == Sensitivity::Sensitive
            || (field.sensitivity == Sensitivity::Personal && !body.include_personal)
            || matches!(field.key.as_str(), "member_id" | "reply_requested")
        {
            return Err(AppError::bad("此欄位不可送往 AI，或尚未同意使用個人資料"));
        }
    }
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    super::authority(&mut tx, actor, Some(site), true).await?;
    let old = load(&mut tx, site).await?;
    if old.revision != revision {
        return Err(AppError::conflict("AI 設定已更新，請重新載入"));
    }
    if body.enabled {
        let id = body
            .provider_id
            .ok_or_else(|| AppError::bad("請先選擇供應商"))?;
        let p = super::providers::load(&mut tx, id).await?;
        if !p.enabled || p.tested_revision != Some(p.revision) {
            return Err(AppError::bad("請先啟用供應商並通過連線測試"));
        }
        let _ = super::providers::client(state, &p)?;
        if body.field_ids.is_empty() {
            return Err(AppError::bad("請明確選擇可送往 AI 的欄位"));
        }
    }
    if body.auto_mode == "auto_template"
        && (!body.enabled || !state.config.workflow.auto_send_enabled || state.mailer.is_none())
    {
        return Err(AppError::bad(
            "自動回覆需要啟用 AI、SMTP 及部署自動寄送開關",
        ));
    }
    let activating = body.auto_mode == "auto_template" && old.auto_mode != "auto_template";
    body.auto_activated_at = if body.auto_mode == "off" {
        None
    } else if activating {
        Some(chrono::Utc::now().timestamp())
    } else {
        old.auto_activated_at
    };
    body.activation_feedback_id = if activating {
        Some(
            sqlx::query_scalar::<_, i64>(
                "SELECT COALESCE(MAX(id),0) FROM feedback WHERE site_id=?",
            )
            .bind(site)
            .fetch_one(&mut *tx)
            .await?,
        )
    } else {
        old.activation_feedback_id
    };
    body.revision = old.revision + 1;
    body.updated_by = Some(actor);
    sqlx::query("INSERT INTO site_ai_settings(site_id,revision,settings_json,updated_by,auto_activated_at,activation_feedback_id,updated_at) VALUES(?,?,?,?,?,?,?) ON CONFLICT(site_id) DO UPDATE SET revision=excluded.revision,settings_json=excluded.settings_json,updated_by=excluded.updated_by,auto_activated_at=excluded.auto_activated_at,activation_feedback_id=excluded.activation_feedback_id,updated_at=excluded.updated_at").bind(site).bind(body.revision).bind(serde_json::to_string(&body)?).bind(actor).bind(body.auto_activated_at).bind(body.activation_feedback_id).bind(db::now()).execute(&mut *tx).await?;
    invalidate(&mut tx, site).await?;
    workflow_audit::record(
        &mut tx,
        Some(site),
        Some(actor),
        "ai_settings",
        site,
        "saved",
        json!({"revision":body.revision,"enabled":body.enabled,"auto_mode":body.auto_mode}),
    )
    .await?;
    tx.commit().await?;
    Ok(body)
}
/// Serialize policy edits and queued-send cancellation with worker transmission claims.
pub async fn invalidate(tx: &mut sqlx::SqliteConnection, site: i64) -> AppResult<()> {
    sqlx::query("UPDATE workflow_jobs SET state='cancelled',token=NULL,lease_until=NULL WHERE site_id=? AND state IN ('queued','running') AND (kind='analysis' OR id IN (SELECT job_id FROM outbound_replies WHERE site_id=? AND mode='auto_template' AND state='queued'))").bind(site).bind(site).execute(&mut *tx).await?;
    sqlx::query("UPDATE ai_runs SET state='superseded',error_code='policy_changed' WHERE site_id=? AND state IN ('queued','running')").bind(site).execute(&mut *tx).await?;
    sqlx::query("UPDATE outbound_replies SET state='cancelled',error_code='policy_changed' WHERE site_id=? AND mode='auto_template' AND state='queued'").bind(site).execute(&mut *tx).await?;
    Ok(())
}
pub async fn touch(tx: &mut sqlx::SqliteConnection, site: i64) -> AppResult<()> {
    sqlx::query("UPDATE site_ai_settings SET revision=revision+1,updated_at=? WHERE site_id=?")
        .bind(db::now())
        .bind(site)
        .execute(&mut *tx)
        .await?;
    invalidate(tx, site).await
}
pub async fn knowledge_valid(
    tx: &mut sqlx::SqliteConnection,
    site: i64,
    ids: &[i64],
) -> AppResult<()> {
    if ids.len() > 5 {
        return Err(AppError::bad("最多引用 5 筆核准資料"));
    }
    for id in ids {
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ai_knowledge k JOIN users u ON u.id=k.approved_by WHERE k.id=? AND k.site_id=? AND k.state='approved' AND u.disabled=0 AND u.account_status='active' AND u.role='admin')").bind(id).bind(site).fetch_one(&mut *tx).await?;
        if !valid {
            return Err(AppError::conflict("引用的知識已變更、停用或尚未核准"));
        }
    }
    Ok(())
}
pub async fn context(state: &AppState, site: i64) -> AppResult<Value> {
    let form = forms::load_version(&state.db, site, None).await?;
    let mut tx = state.db.begin().await?;
    let settings = load(&mut tx, site).await?;
    let members:Vec<(i64,String)>=sqlx::query_as("SELECT u.id,u.display_name FROM users u WHERE u.disabled=0 AND u.account_status='active' AND (u.role='admin' OR EXISTS(SELECT 1 FROM site_members m WHERE m.site_id=? AND m.user_id=u.id)) ORDER BY u.id").bind(site).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(
        json!({"settings":settings,"providers":super::providers::list(state).await?,"fields":form.schema.fields,"members":members.into_iter().map(|(id,name)|json!({"id":id,"display_name":name})).collect::<Vec<_>>(),"capabilities":{"encryption_ready":state.config.workflow.secret_key.is_some(),"smtp_ready":state.mailer.is_some(),"auto_send_enabled":state.config.workflow.auto_send_enabled}}),
    )
}
