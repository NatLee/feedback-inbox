use crate::{ai::providers, auth::Admin, error::AppResult, state::AppState};
use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
#[derive(Deserialize)]
pub struct Revision {
    pub expected_revision: i64,
}
pub async fn provider_list(State(s): State<AppState>, _admin: Admin) -> AppResult<Json<Value>> {
    Ok(Json(
        json!({"items":providers::list(&s).await?,"encryption_ready":s.config.workflow.secret_key.is_some(),"allowed_ollama_origins":s.config.workflow.allowed_ollama_origins,"auto_send_enabled":s.config.workflow.auto_send_enabled,"smtp_ready":s.mailer.is_some()}),
    ))
}
pub async fn provider_create(
    State(s): State<AppState>,
    Admin(u): Admin,
    Json(b): Json<providers::ProviderBody>,
) -> AppResult<Json<providers::Provider>> {
    Ok(Json(providers::save(&s, u.id, None, b).await?))
}
pub async fn provider_update(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path(id): Path<i64>,
    Json(b): Json<providers::ProviderBody>,
) -> AppResult<Json<providers::Provider>> {
    Ok(Json(providers::save(&s, u.id, Some(id), b).await?))
}
pub async fn provider_test(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path(id): Path<i64>,
    Json(b): Json<Revision>,
) -> AppResult<Json<providers::Provider>> {
    Ok(Json(
        providers::test(&s, u.id, id, b.expected_revision).await?,
    ))
}

pub async fn site_settings(
    State(s): State<AppState>,
    u: crate::auth::CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    super::sites::ensure_visible(&s, &u, id).await?;
    Ok(Json(crate::ai::policy::context(&s, id).await?))
}
#[derive(Deserialize)]
pub struct SettingsBody {
    expected_revision: i64,
    #[serde(flatten)]
    settings: crate::ai::policy::Settings,
}
pub async fn site_settings_update(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path(id): Path<i64>,
    Json(b): Json<SettingsBody>,
) -> AppResult<Json<crate::ai::policy::Settings>> {
    Ok(Json(
        crate::ai::policy::save(&s, id, u.id, b.expected_revision, b.settings).await?,
    ))
}

fn content_kind(kind: &str) -> AppResult<crate::ai::knowledge::Kind> {
    match kind {
        "knowledge" => Ok(crate::ai::knowledge::Kind::Knowledge),
        "templates" => Ok(crate::ai::knowledge::Kind::Template),
        _ => Err(crate::error::AppError::not_found("未知資料類型")),
    }
}
pub async fn content_list(
    State(s): State<AppState>,
    u: crate::auth::CurrentUser,
    Path((id, kind)): Path<(i64, String)>,
) -> AppResult<Json<Value>> {
    super::sites::ensure_visible(&s, &u, id).await?;
    Ok(Json(
        crate::ai::knowledge::list(&s, id, content_kind(&kind)?).await?,
    ))
}
pub async fn content_create(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path((id, kind)): Path<(i64, String)>,
    Json(b): Json<crate::ai::knowledge::Body>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        crate::ai::knowledge::save(&s, id, u.id, None, content_kind(&kind)?, b).await?,
    ))
}
pub async fn content_update(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path((id, kind, item)): Path<(i64, String, i64)>,
    Json(b): Json<crate::ai::knowledge::Body>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        crate::ai::knowledge::save(&s, id, u.id, Some(item), content_kind(&kind)?, b).await?,
    ))
}
pub async fn content_transition(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path((id, kind, item, action)): Path<(i64, String, i64, String)>,
    Json(b): Json<Revision>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        crate::ai::knowledge::transition(
            &s,
            id,
            u.id,
            item,
            content_kind(&kind)?,
            b.expected_revision,
            &action,
        )
        .await?,
    ))
}

async fn feedback_site(s: &AppState, u: &crate::auth::CurrentUser, id: i64) -> AppResult<i64> {
    let site: Option<i64> = sqlx::query_scalar("SELECT site_id FROM feedback WHERE id=?")
        .bind(id)
        .fetch_optional(&s.db)
        .await?;
    let site = site.ok_or_else(|| crate::error::AppError::not_found("找不到訊息"))?;
    super::sites::ensure_visible(s, u, site).await?;
    Ok(site)
}
pub async fn analyze(
    State(s): State<AppState>,
    u: crate::auth::CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    feedback_site(&s, &u, id).await?;
    let mut tx = s.db.begin_with("BEGIN IMMEDIATE").await?;
    let value = crate::ai::analysis::enqueue(&mut tx, &s, id, u.id).await?;
    tx.commit().await?;
    Ok(Json(value))
}
pub async fn feedback_ai(
    State(s): State<AppState>,
    u: crate::auth::CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    use sqlx::Row;
    feedback_site(&s, &u, id).await?;
    let rows = sqlx::query("SELECT * FROM ai_runs WHERE feedback_id=? ORDER BY id DESC LIMIT 20")
        .bind(id)
        .fetch_all(&s.db)
        .await?;
    let runs:Vec<Value>=rows.into_iter().map(|r|json!({"id":r.get::<i64,_>("id"),"state":r.get::<String,_>("state"),"provider_id":r.get::<i64,_>("provider_id"),"model":r.get::<Option<String>,_>("model"),"error_code":r.get::<Option<String>,_>("error_code"),"error_message":r.get::<Option<String>,_>("error_message"),"created_at":r.get::<String,_>("created_at"),"output":r.get::<Option<String>,_>("output_json").and_then(|v|serde_json::from_str::<Value>(&v).ok()),"sources":serde_json::from_str::<Value>(r.get::<&str,_>("input_json")).unwrap_or_default()["fields"],"usage":r.get::<Option<String>,_>("usage_json").and_then(|v|serde_json::from_str::<Value>(&v).ok())})).collect();
    let proposal=sqlx::query("SELECT * FROM classification_proposals WHERE feedback_id=? ORDER BY id DESC LIMIT 1").bind(id).fetch_optional(&s.db).await?.map(|r|json!({"id":r.get::<i64,_>("id"),"revision":r.get::<i64,_>("revision"),"state":r.get::<String,_>("state"),"category":r.get::<String,_>("category"),"tags":serde_json::from_str::<Value>(r.get::<&str,_>("tags_json")).unwrap_or(json!([])),"urgency":r.get::<String,_>("urgency"),"owner_id":r.get::<Option<i64>,_>("owner_id")}));
    let decision=sqlx::query("SELECT gate,explanation FROM auto_reply_decisions WHERE feedback_id=?").bind(id).fetch_optional(&s.db).await?.map(|r|json!({"gate":r.get::<String,_>("gate"),"explanation":r.get::<String,_>("explanation")}));
    let mut tx = s.db.begin().await?;
    let draft = crate::ai::replies::draft(&mut tx, id).await?;
    let outbound: Vec<crate::ai::replies::Outbound> = sqlx::query_as(
        "SELECT * FROM outbound_replies WHERE feedback_id=? ORDER BY id DESC LIMIT 50",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"runs":runs,"proposal":proposal,"draft":draft,"outbound":outbound,"auto_decision":decision}),
    ))
}
pub async fn cancel_run(
    State(s): State<AppState>,
    u: crate::auth::CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let mut tx = s.db.begin_with("BEGIN IMMEDIATE").await?;
    let row: Option<(i64, i64, i64)> =
        sqlx::query_as("SELECT site_id,feedback_id,job_id FROM ai_runs WHERE id=?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    let (site, _, job) = row.ok_or_else(|| crate::error::AppError::not_found("找不到分析"))?;
    crate::ai::authority(&mut tx, u.id, Some(site), false).await?;
    sqlx::query("UPDATE workflow_jobs SET state='cancelled',token=NULL,lease_until=NULL WHERE id=? AND state IN ('queued','running')").bind(job).execute(&mut *tx).await?;
    sqlx::query(
        "UPDATE ai_runs SET state='cancelled' WHERE id=? AND state IN ('queued','running')",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    crate::workflow_audit::record(
        &mut tx,
        Some(site),
        Some(u.id),
        "ai_run",
        id,
        "cancelled",
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
pub struct Classification {
    expected_revision: i64,
    decision: String,
    category: Option<String>,
    tags: Option<Vec<String>>,
    urgency: Option<String>,
    owner_id: Option<i64>,
}
pub async fn classification(
    State(s): State<AppState>,
    u: crate::auth::CurrentUser,
    Path(id): Path<i64>,
    Json(b): Json<Classification>,
) -> AppResult<Json<Value>> {
    use crate::error::AppError;
    use sqlx::Row;
    let mut tx = s.db.begin_with("BEGIN IMMEDIATE").await?;
    let source = crate::ai::analysis::source(&mut tx, id).await?;
    crate::ai::authority(&mut tx, u.id, Some(source.site_id), false).await?;
    let row=sqlx::query("SELECT p.*,r.source_hash,r.policy_revision,r.state run_state FROM classification_proposals p JOIN ai_runs r ON r.id=p.run_id WHERE p.feedback_id=? ORDER BY p.id DESC LIMIT 1").bind(id).fetch_optional(&mut *tx).await?.ok_or_else(||AppError::not_found("找不到分類建議"))?;
    if row.get::<i64, _>("revision") != b.expected_revision
        || row.get::<String, _>("state") != "pending"
    {
        return Err(AppError::conflict("分類建議已處理或更新"));
    }
    let settings = crate::ai::policy::load(&mut tx, source.site_id).await?;
    if row.get::<String, _>("run_state") != "succeeded"
        || row.get::<String, _>("source_hash") != crate::ai::analysis::source_hash(&source)
        || row.get::<i64, _>("policy_revision") != settings.revision
    {
        return Err(AppError::conflict("來源或 AI 設定已變更，請重新分析"));
    }
    let category = b.category.unwrap_or_else(|| row.get("category"));
    let tags = b.tags.unwrap_or_else(|| {
        serde_json::from_str(row.get::<&str, _>("tags_json")).unwrap_or_default()
    });
    let urgency = b.urgency.unwrap_or_else(|| row.get("urgency"));
    let owner = b.owner_id;
    if !settings.categories.contains(&category)
        || tags.iter().any(|t| !settings.tags.contains(t))
        || !matches!(urgency.as_str(), "low" | "normal" | "high")
    {
        return Err(AppError::bad("分類或標籤不在允許範圍內"));
    }
    if let Some(owner) = owner {
        crate::ai::authority(&mut tx, owner, Some(source.site_id), false).await?;
    }
    let target = match b.decision.as_str() {
        "accept" => "accepted",
        "reject" => "rejected",
        _ => return Err(AppError::bad("處理方式不符")),
    };
    let proposal: i64 = row.get("id");
    sqlx::query("UPDATE classification_proposals SET state=?,revision=revision+1,category=?,tags_json=?,urgency=?,owner_id=? WHERE id=?").bind(target).bind(&category).bind(serde_json::to_string(&tags)?).bind(&urgency).bind(owner).bind(proposal).execute(&mut *tx).await?;
    if target == "accepted" {
        sqlx::query("UPDATE feedback SET category=?,tags_json=?,urgency=?,owner_id=? WHERE id=?")
            .bind(&category)
            .bind(serde_json::to_string(&tags)?)
            .bind(&urgency)
            .bind(owner)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    crate::workflow_audit::record(
        &mut tx,
        Some(source.site_id),
        Some(u.id),
        "classification",
        proposal,
        target,
        json!({"revision":b.expected_revision+1}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"id":proposal,"state":target,"revision":b.expected_revision+1,"category":category,"tags":tags,"urgency":urgency,"owner_id":owner}),
    ))
}

pub async fn draft_create(
    State(s): State<AppState>,
    u: crate::auth::CurrentUser,
    Path(id): Path<i64>,
    Json(b): Json<crate::ai::replies::DraftBody>,
) -> AppResult<Json<crate::ai::replies::Draft>> {
    feedback_site(&s, &u, id).await?;
    Ok(Json(crate::ai::replies::save(&s, id, u.id, b, true).await?))
}
pub async fn draft_update(
    State(s): State<AppState>,
    u: crate::auth::CurrentUser,
    Path(id): Path<i64>,
    Json(b): Json<crate::ai::replies::DraftBody>,
) -> AppResult<Json<crate::ai::replies::Draft>> {
    feedback_site(&s, &u, id).await?;
    Ok(Json(
        crate::ai::replies::save(&s, id, u.id, b, false).await?,
    ))
}
pub async fn draft_reject(
    State(s): State<AppState>,
    u: crate::auth::CurrentUser,
    Path(id): Path<i64>,
    Json(b): Json<Revision>,
) -> AppResult<Json<crate::ai::replies::Draft>> {
    feedback_site(&s, &u, id).await?;
    Ok(Json(
        crate::ai::replies::reject(&s, id, u.id, b.expected_revision).await?,
    ))
}
pub async fn reply_send(
    State(s): State<AppState>,
    u: crate::auth::CurrentUser,
    Path(id): Path<i64>,
    Json(b): Json<crate::ai::replies::Approval>,
) -> AppResult<Json<crate::ai::replies::Outbound>> {
    feedback_site(&s, &u, id).await?;
    Ok(Json(crate::ai::replies::approve(&s, id, u.id, b).await?))
}
pub async fn outbound_cancel(
    State(s): State<AppState>,
    u: crate::auth::CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<Json<crate::ai::replies::Outbound>> {
    Ok(Json(crate::ai::replies::cancel(&s, id, u.id).await?))
}
pub async fn outbound_reconcile(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path(id): Path<i64>,
    Json(b): Json<crate::ai::replies::Reconciliation>,
) -> AppResult<Json<crate::ai::replies::Outbound>> {
    Ok(Json(crate::ai::replies::reconcile(&s, id, u.id, b).await?))
}
pub async fn review(
    State(s): State<AppState>,
    u: crate::auth::CurrentUser,
) -> AppResult<Json<Value>> {
    use sqlx::Row;
    let rows=sqlx::query("SELECT f.id,f.site_id,s.name site_name,f.name,f.created_at,(SELECT state FROM reply_drafts WHERE feedback_id=f.id ORDER BY revision DESC LIMIT 1) draft_state,(SELECT state FROM ai_runs WHERE feedback_id=f.id ORDER BY id DESC LIMIT 1) run_state,(SELECT state FROM outbound_replies WHERE feedback_id=f.id ORDER BY id DESC LIMIT 1) outbound_state,(SELECT explanation FROM auto_reply_decisions WHERE feedback_id=f.id) fallback_reason FROM feedback f JOIN sites s ON s.id=f.site_id WHERE f.is_test=0 AND f.status!='spam' AND (? OR EXISTS(SELECT 1 FROM site_members m WHERE m.site_id=f.site_id AND m.user_id=?)) AND (EXISTS(SELECT 1 FROM reply_drafts d WHERE d.feedback_id=f.id) OR EXISTS(SELECT 1 FROM ai_runs r WHERE r.feedback_id=f.id)) ORDER BY f.id DESC LIMIT 200").bind(u.is_admin()).bind(u.id).fetch_all(&s.db).await?;
    let items:Vec<Value>=rows.into_iter().map(|r|json!({"feedback_id":r.get::<i64,_>("id"),"site_id":r.get::<i64,_>("site_id"),"site_name":r.get::<String,_>("site_name"),"sender":r.get::<String,_>("name"),"created_at":r.get::<String,_>("created_at"),"draft_state":r.get::<Option<String>,_>("draft_state"),"run_state":r.get::<Option<String>,_>("run_state"),"outbound_state":r.get::<Option<String>,_>("outbound_state"),"fallback_reason":r.get::<Option<String>,_>("fallback_reason")})).collect();
    Ok(Json(json!({"items":items})))
}
pub async fn rules_list(
    State(s): State<AppState>,
    u: crate::auth::CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    super::sites::ensure_visible(&s, &u, id).await?;
    Ok(Json(crate::ai::auto_reply::list(&s, id).await?))
}
pub async fn rule_create(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path(id): Path<i64>,
    Json(b): Json<crate::ai::auto_reply::Rule>,
) -> AppResult<Json<crate::ai::auto_reply::Rule>> {
    Ok(Json(
        crate::ai::auto_reply::save(&s, id, u.id, None, b).await?,
    ))
}
pub async fn rule_update(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path((id, item)): Path<(i64, i64)>,
    Json(b): Json<crate::ai::auto_reply::Rule>,
) -> AppResult<Json<crate::ai::auto_reply::Rule>> {
    Ok(Json(
        crate::ai::auto_reply::save(&s, id, u.id, Some(item), b).await?,
    ))
}
pub async fn rule_remove(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path((id, item)): Path<(i64, i64)>,
) -> AppResult<Json<Value>> {
    crate::ai::auto_reply::remove(&s, id, u.id, item).await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
pub struct DryRun {
    feedback_id: i64,
}
pub async fn auto_dry_run(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path(id): Path<i64>,
    Json(b): Json<DryRun>,
) -> AppResult<Json<crate::ai::auto_reply::Decision>> {
    let mut tx = s.db.begin().await?;
    crate::ai::authority(&mut tx, u.id, Some(id), true).await?;
    let source = crate::ai::analysis::source(&mut tx, b.feedback_id).await?;
    if source.site_id != id {
        return Err(crate::error::AppError::not_found("找不到網站訊息"));
    }
    let result = crate::ai::auto_reply::evaluate(&mut tx, &s, b.feedback_id, None).await?;
    tx.commit().await?;
    Ok(Json(result))
}

pub async fn jobs_list(
    State(s): State<AppState>,
    u: crate::auth::CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    use sqlx::Row;
    super::sites::ensure_visible(&s, &u, id).await?;
    let rows=sqlx::query("SELECT id,kind,state,attempt,error_code,created_at FROM workflow_jobs WHERE site_id=? ORDER BY id DESC LIMIT 100").bind(id).fetch_all(&s.db).await?;
    Ok(Json(
        json!({"items":rows.into_iter().map(|r|json!({"id":r.get::<i64,_>("id"),"kind":r.get::<String,_>("kind"),"state":r.get::<String,_>("state"),"attempt":r.get::<i64,_>("attempt"),"error_code":r.get::<Option<String>,_>("error_code"),"created_at":r.get::<String,_>("created_at")})).collect::<Vec<_>>()}),
    ))
}
