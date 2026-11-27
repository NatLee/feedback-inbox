//! Exact-content approval and durable visitor replies; internal notes are never mailed.
use super::{analysis, policy};
use crate::{
    auth, db,
    error::{AppError, AppResult},
    jobs,
    state::AppState,
    workflow_audit,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;
#[derive(Deserialize)]
pub struct DraftBody {
    pub expected_revision: Option<i64>,
    pub recipient: String,
    pub subject: String,
    pub body: String,
    #[serde(default)]
    pub knowledge_ids: Vec<i64>,
}
#[derive(FromRow, Serialize, Clone)]
pub struct Draft {
    pub id: i64,
    pub feedback_id: i64,
    pub revision: i64,
    pub origin: String,
    pub run_id: Option<i64>,
    pub state: String,
    pub recipient: String,
    pub subject: String,
    pub body: String,
    pub content_hash: String,
    #[serde(skip)]
    pub knowledge_ids_json: String,
    #[sqlx(skip)]
    pub knowledge_ids: Vec<i64>,
    #[serde(skip)]
    pub source_hash: String,
    pub created_at: String,
}
#[derive(FromRow, Serialize, Clone)]
pub struct Outbound {
    pub id: i64,
    pub feedback_id: i64,
    pub site_id: i64,
    pub job_id: Option<i64>,
    pub mode: String,
    pub state: String,
    pub draft_id: Option<i64>,
    pub template_id: Option<i64>,
    pub rule_id: Option<i64>,
    pub actor_id: Option<i64>,
    pub policy_revision: Option<i64>,
    pub provider_revision: Option<i64>,
    #[serde(skip)]
    pub source_hash: String,
    pub recipient: String,
    pub subject: String,
    pub body: String,
    #[serde(skip)]
    pub knowledge_ids_json: String,
    pub content_hash: String,
    pub message_id: String,
    #[serde(skip)]
    pub request_hash: String,
    pub created_at: String,
    pub sent_at: Option<String>,
    pub error_code: Option<String>,
}
pub async fn draft(tx: &mut sqlx::SqliteConnection, feedback: i64) -> AppResult<Option<Draft>> {
    let mut d = sqlx::query_as::<_, Draft>(
        "SELECT * FROM reply_drafts WHERE feedback_id=? ORDER BY revision DESC LIMIT 1",
    )
    .bind(feedback)
    .fetch_optional(tx)
    .await?;
    if let Some(d) = d.as_mut() {
        d.knowledge_ids = serde_json::from_str(&d.knowledge_ids_json)?
    }
    Ok(d)
}
pub async fn outbound(tx: &mut sqlx::SqliteConnection, id: i64) -> AppResult<Outbound> {
    sqlx::query_as("SELECT * FROM outbound_replies WHERE id=?")
        .bind(id)
        .fetch_optional(tx)
        .await?
        .ok_or_else(|| AppError::not_found("找不到寄送紀錄"))
}
pub fn validate_content(recipient: &str, subject: &str, body: &str) -> AppResult<()> {
    if !crate::intake::looks_like_email(recipient)
        || recipient.len() > 254
        || recipient.contains([',', ';', '<', '>', '\r', '\n'])
        || recipient.chars().any(char::is_control)
    {
        return Err(AppError::bad("請指定一個有效收件 Email"));
    }
    if subject.trim().is_empty()
        || subject.chars().count() > 200
        || subject.chars().any(char::is_control)
        || body.trim().is_empty()
        || body.chars().count() > 10000
        || body.contains('\0')
    {
        return Err(AppError::bad(
            "主旨需為 1–200 字且不能換行，內容需為 1–10000 字",
        ));
    }
    Ok(())
}
pub fn content_hash(recipient: &str, subject: &str, body: &str, knowledge: &[i64]) -> String {
    auth::sha256_hex(
        &json!({"recipient":recipient,"subject":subject,"body":body,"knowledge_ids":knowledge})
            .to_string(),
    )
}
async fn cancel_queued(
    tx: &mut sqlx::SqliteConnection,
    feedback: i64,
    reason: &str,
) -> AppResult<()> {
    let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM outbound_replies WHERE feedback_id=? AND state IN ('sending','sent','delivery_unknown'))").bind(feedback).fetch_one(&mut *tx).await?;
    if blocked {
        return Err(AppError::conflict(
            "此訊息已有寄送中、已寄出或結果不明的回覆，請先處理寄送紀錄",
        ));
    }
    sqlx::query("UPDATE workflow_jobs SET state='cancelled',token=NULL,lease_until=NULL WHERE id IN (SELECT job_id FROM outbound_replies WHERE feedback_id=? AND state='queued')").bind(feedback).execute(&mut *tx).await?;
    sqlx::query("UPDATE outbound_replies SET state='cancelled',error_code=? WHERE feedback_id=? AND state='queued'").bind(reason).bind(feedback).execute(&mut *tx).await?;
    Ok(())
}
pub async fn save(
    state: &AppState,
    feedback: i64,
    actor: i64,
    body: DraftBody,
    create: bool,
) -> AppResult<Draft> {
    validate_content(&body.recipient, &body.subject, &body.body)?;
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let source = analysis::source(&mut tx, feedback).await?;
    super::authority(&mut tx, actor, Some(source.site_id), false).await?;
    if source.is_test || source.status == "spam" {
        return Err(AppError::bad("測試或垃圾訊息不能建立對外回覆"));
    }
    let old = draft(&mut tx, feedback).await?;
    if create && old.is_some() {
        return Err(AppError::conflict("已有回覆草稿，請編輯現有版本"));
    }
    if !create && old.as_ref().map(|d| d.revision) != body.expected_revision {
        return Err(AppError::conflict("回覆草稿已更新，請重新載入"));
    }
    policy::knowledge_valid(&mut tx, source.site_id, &body.knowledge_ids).await?;
    cancel_queued(&mut tx, feedback, "draft_edited").await?;
    let revision = old.map(|d| d.revision + 1).unwrap_or(1);
    sqlx::query("UPDATE reply_drafts SET state='superseded' WHERE feedback_id=? AND state IN ('draft','approved')").bind(feedback).execute(&mut *tx).await?;
    insert_draft(&mut tx, &source, actor, revision, "human", None, &body).await?;
    workflow_audit::record(
        &mut tx,
        Some(source.site_id),
        Some(actor),
        "reply_draft",
        feedback,
        "saved",
        json!({"revision":revision}),
    )
    .await?;
    let result = draft(&mut tx, feedback).await?.unwrap();
    tx.commit().await?;
    Ok(result)
}
#[allow(clippy::too_many_arguments)]
async fn insert_draft(
    tx: &mut sqlx::SqliteConnection,
    source: &analysis::Source,
    actor: i64,
    revision: i64,
    origin: &str,
    run: Option<i64>,
    body: &DraftBody,
) -> AppResult<()> {
    let hash = content_hash(
        &body.recipient,
        &body.subject,
        &body.body,
        &body.knowledge_ids,
    );
    sqlx::query("INSERT INTO reply_drafts(feedback_id,revision,origin,run_id,recipient,subject,body,content_hash,knowledge_ids_json,source_hash,created_by,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)").bind(source.id).bind(revision).bind(origin).bind(run).bind(&body.recipient).bind(&body.subject).bind(&body.body).bind(hash).bind(serde_json::to_string(&body.knowledge_ids)?).bind(analysis::source_hash(source)).bind(actor).bind(db::now()).execute(tx).await?;
    Ok(())
}
/// Analysis may propose a first draft; it never overwrites an existing human review.
pub async fn from_analysis(
    tx: &mut sqlx::SqliteConnection,
    run: &analysis::Run,
    suggestion: &analysis::SuggestedDraft,
) -> AppResult<()> {
    if draft(tx, run.feedback_id).await?.is_some() {
        return Ok(());
    }
    let source = analysis::source(tx, run.feedback_id).await?;
    if validate_content(&source.email, &suggestion.subject, &suggestion.body).is_err() {
        return Ok(());
    }
    policy::knowledge_valid(tx, run.site_id, &suggestion.knowledge_ids).await?;
    insert_draft(
        tx,
        &source,
        run.actor_id
            .ok_or_else(|| AppError::forbidden("執行者已移除"))?,
        1,
        "ai",
        Some(run.id),
        &DraftBody {
            expected_revision: None,
            recipient: source.email.clone(),
            subject: suggestion.subject.clone(),
            body: suggestion.body.clone(),
            knowledge_ids: suggestion.knowledge_ids.clone(),
        },
    )
    .await
}
#[derive(Deserialize)]
pub struct Approval {
    pub expected_revision: i64,
    pub content_hash: String,
    pub idempotency_key: String,
}
pub async fn approve(
    state: &AppState,
    feedback: i64,
    actor: i64,
    b: Approval,
) -> AppResult<Outbound> {
    if state.mailer.is_none() {
        return Err(AppError::bad("尚未設定 SMTP，無法核准寄出"));
    }
    if !(16..=80).contains(&b.idempotency_key.len())
        || !b
            .idempotency_key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
    {
        return Err(AppError::bad("寄送識別碼格式不符"));
    }
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let source = analysis::source(&mut tx, feedback).await?;
    super::authority(&mut tx, actor, Some(source.site_id), false).await?;
    let request_hash=auth::sha256_hex(&json!({"feedback":feedback,"revision":b.expected_revision,"content_hash":b.content_hash,"actor":actor}).to_string());
    let old: Option<Outbound> =
        sqlx::query_as("SELECT * FROM outbound_replies WHERE site_id=? AND idempotency_key=?")
            .bind(source.site_id)
            .bind(&b.idempotency_key)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(old) = old {
        if old.request_hash != request_hash {
            return Err(AppError::conflict("寄送識別碼已用於不同內容"));
        }
        tx.commit().await?;
        return Ok(old);
    }
    let d = draft(&mut tx, feedback)
        .await?
        .ok_or_else(|| AppError::not_found("請先儲存回覆草稿"))?;
    if d.revision != b.expected_revision || d.content_hash != b.content_hash || d.state != "draft" {
        return Err(AppError::conflict("草稿已更新或已處理，請重新檢查"));
    }
    validate_content(&d.recipient, &d.subject, &d.body)?;
    if source.is_test || source.status == "spam" || d.source_hash != analysis::source_hash(&source)
    {
        return Err(AppError::conflict("來源訊息已變更，請重新儲存與審核草稿"));
    }
    policy::knowledge_valid(&mut tx, source.site_id, &d.knowledge_ids).await?;
    let settings = policy::load(&mut tx, source.site_id).await?;
    let provider_revision = if let Some(run) = d.run_id {
        let run: analysis::Run = sqlx::query_as("SELECT * FROM ai_runs WHERE id=?")
            .bind(run)
            .fetch_one(&mut *tx)
            .await?;
        super::analysis::recheck(&mut tx, state, &run).await?;
        Some(run.provider_revision)
    } else {
        None
    };
    let existing:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM outbound_replies WHERE feedback_id=? AND state IN ('queued','sending','sent','delivery_unknown'))").bind(feedback).fetch_one(&mut *tx).await?;
    if existing {
        return Err(AppError::conflict("此訊息已有待寄送或已寄送的回覆"));
    }
    let message_id = format!("<fi-{}@feedback-inbox.invalid>", auth::random_hex(16));
    let id=sqlx::query("INSERT INTO outbound_replies(feedback_id,site_id,mode,draft_id,actor_id,policy_revision,provider_revision,source_hash,recipient,subject,body,knowledge_ids_json,content_hash,message_id,idempotency_key,request_hash,created_at) VALUES(?,?,'human',?,?,?,?,?,?,?,?,?,?,?,?,?,?)").bind(feedback).bind(source.site_id).bind(d.id).bind(actor).bind(settings.revision).bind(provider_revision).bind(&d.source_hash).bind(&d.recipient).bind(&d.subject).bind(&d.body).bind(&d.knowledge_ids_json).bind(&d.content_hash).bind(message_id).bind(&b.idempotency_key).bind(request_hash).bind(db::now()).execute(&mut *tx).await?.last_insert_rowid();
    let job = jobs::enqueue(
        &mut tx,
        source.site_id,
        "reply",
        json!({"outbound_id":id}),
        &format!("reply:{id}"),
    )
    .await?;
    sqlx::query("UPDATE outbound_replies SET job_id=? WHERE id=?")
        .bind(job)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE reply_drafts SET state='approved' WHERE id=?")
        .bind(d.id)
        .execute(&mut *tx)
        .await?;
    workflow_audit::record(
        &mut tx,
        Some(source.site_id),
        Some(actor),
        "outbound",
        id,
        "approved",
        json!({"draft_id":d.id,"content_hash":d.content_hash}),
    )
    .await?;
    let result = outbound(&mut tx, id).await?;
    tx.commit().await?;
    Ok(result)
}
pub async fn reject(
    state: &AppState,
    feedback: i64,
    actor: i64,
    revision: i64,
) -> AppResult<Draft> {
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let source = analysis::source(&mut tx, feedback).await?;
    super::authority(&mut tx, actor, Some(source.site_id), false).await?;
    let d = draft(&mut tx, feedback)
        .await?
        .ok_or_else(|| AppError::not_found("找不到草稿"))?;
    if d.revision != revision || !matches!(d.state.as_str(), "draft" | "approved") {
        return Err(AppError::conflict("草稿已更新或已處理"));
    }
    cancel_queued(&mut tx, feedback, "draft_rejected").await?;
    sqlx::query("UPDATE reply_drafts SET state='rejected' WHERE id=?")
        .bind(d.id)
        .execute(&mut *tx)
        .await?;
    workflow_audit::record(
        &mut tx,
        Some(source.site_id),
        Some(actor),
        "reply_draft",
        d.id,
        "rejected",
        json!({"revision":revision}),
    )
    .await?;
    let result = draft(&mut tx, feedback).await?.unwrap();
    tx.commit().await?;
    Ok(result)
}
pub async fn cancel(state: &AppState, id: i64, actor: i64) -> AppResult<Outbound> {
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let o = outbound(&mut tx, id).await?;
    super::authority(&mut tx, actor, Some(o.site_id), false).await?;
    if o.state != "queued" {
        return Err(AppError::conflict("只有尚未開始傳送的回覆可以取消"));
    }
    cancel_queued(&mut tx, o.feedback_id, "user_cancelled").await?;
    if let Some(d) = o.draft_id {
        sqlx::query("UPDATE reply_drafts SET state='draft' WHERE id=? AND state='approved'")
            .bind(d)
            .execute(&mut *tx)
            .await?;
    }
    workflow_audit::record(
        &mut tx,
        Some(o.site_id),
        Some(actor),
        "outbound",
        id,
        "cancelled",
        json!({}),
    )
    .await?;
    let result = outbound(&mut tx, id).await?;
    tx.commit().await?;
    Ok(result)
}
#[derive(Deserialize)]
pub struct Reconciliation {
    pub resolution: String,
    pub note: String,
}
pub async fn reconcile(
    state: &AppState,
    id: i64,
    actor: i64,
    b: Reconciliation,
) -> AppResult<Outbound> {
    if b.note.trim().is_empty() || b.note.chars().count() > 1000 {
        return Err(AppError::bad("請提供 1–1000 字的人工查證紀錄"));
    }
    let target = match b.resolution.as_str() {
        "confirmed_sent" => "sent",
        "confirmed_not_sent" => "failed",
        _ => return Err(AppError::bad("查證結果格式不符")),
    };
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let o = outbound(&mut tx, id).await?;
    super::authority(&mut tx, actor, Some(o.site_id), true).await?;
    if o.state != "delivery_unknown" {
        return Err(AppError::conflict("只有結果不明的寄送可人工查證"));
    }
    sqlx::query("UPDATE outbound_replies SET state=?,sent_at=?,error_code=? WHERE id=?")
        .bind(target)
        .bind((target == "sent").then(db::now))
        .bind((target == "failed").then_some("confirmed_not_sent"))
        .bind(id)
        .execute(&mut *tx)
        .await?;
    // Keep arbitrary operator text out of audit metadata. Its digest proves which statement was acknowledged.
    workflow_audit::record(
        &mut tx,
        Some(o.site_id),
        Some(actor),
        "outbound",
        id,
        "reconciled",
        json!({"resolution":b.resolution,"note_hash":auth::sha256_hex(&b.note)}),
    )
    .await?;
    let result = outbound(&mut tx, id).await?;
    tx.commit().await?;
    Ok(result)
}
async fn recheck(tx: &mut sqlx::SqliteConnection, state: &AppState, o: &Outbound) -> AppResult<()> {
    let actor = o
        .actor_id
        .ok_or_else(|| AppError::forbidden("核准者已移除"))?;
    super::authority(tx, actor, Some(o.site_id), o.mode == "auto_template").await?;
    let source = analysis::source(tx, o.feedback_id).await?;
    let active: bool = sqlx::query_scalar("SELECT active FROM sites WHERE id=?")
        .bind(o.site_id)
        .fetch_one(&mut *tx)
        .await?;
    if !active
        || source.is_test
        || source.status == "spam"
        || analysis::source_hash(&source) != o.source_hash
    {
        return Err(AppError::conflict("網站或來源訊息已變更"));
    }
    let settings = policy::load(tx, o.site_id).await?;
    if Some(settings.revision) != o.policy_revision {
        return Err(AppError::conflict("回覆政策已變更，需重新核准"));
    }
    let ids: Vec<i64> = serde_json::from_str(&o.knowledge_ids_json)?;
    policy::knowledge_valid(tx, o.site_id, &ids).await?;
    if let Some(draft_id) = o.draft_id {
        let d = draft(tx, o.feedback_id)
            .await?
            .ok_or_else(|| AppError::conflict("草稿已移除"))?;
        if d.id != draft_id || d.state != "approved" || d.content_hash != o.content_hash {
            return Err(AppError::conflict("核准草稿已變更"));
        }
        if let Some(run_id) = d.run_id {
            let run: analysis::Run = sqlx::query_as("SELECT * FROM ai_runs WHERE id=?")
                .bind(run_id)
                .fetch_one(&mut *tx)
                .await?;
            analysis::recheck(tx, state, &run).await?;
        }
    }
    if o.mode == "auto_template" {
        let decision = super::auto_reply::evaluate(tx, state, o.feedback_id, Some(o)).await?;
        let Some(eligible) = decision.approved else {
            return Err(AppError::conflict(decision.explanation));
        };
        if eligible.recipient != o.recipient
            || eligible.subject != o.subject
            || eligible.body != o.body
        {
            return Err(AppError::conflict("範本內容已變更"));
        }
    }
    validate_content(&o.recipient, &o.subject, &o.body)?;
    if content_hash(&o.recipient, &o.subject, &o.body, &ids) != o.content_hash {
        return Err(AppError::conflict("寄送內容雜湊不符"));
    }
    Ok(())
}
pub async fn execute(state: &AppState, claim: &jobs::Claim) -> AppResult<()> {
    let id = serde_json::from_str::<Value>(&claim.payload_json)?["outbound_id"]
        .as_i64()
        .ok_or_else(|| AppError::bad("寄送工作格式不符"))?;
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    jobs::current(&mut tx, claim).await?;
    let o = outbound(&mut tx, id).await?;
    if o.state != "queued" {
        return Err(AppError::conflict("寄送已開始或停止"));
    }
    if state.mailer.is_none() || recheck(&mut tx, state, &o).await.is_err() {
        sqlx::query("UPDATE outbound_replies SET state='cancelled',error_code='authority_policy_or_source_changed' WHERE id=? AND state='queued'").bind(id).execute(&mut *tx).await?;
        sqlx::query("UPDATE workflow_jobs SET state='cancelled',token=NULL,lease_until=NULL WHERE id=? AND token=?").bind(claim.id).bind(&claim.token).execute(&mut *tx).await?;
        tx.commit().await?;
        return Ok(());
    }
    // The same write lock serializes pause/edit/cancel with the transmission boundary.
    sqlx::query("UPDATE outbound_replies SET state='sending' WHERE id=? AND state='queued'")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    workflow_audit::record(
        &mut tx,
        Some(o.site_id),
        o.actor_id,
        "outbound",
        id,
        "transmission_started",
        json!({"message_id":o.message_id,"content_hash":o.content_hash}),
    )
    .await?;
    tx.commit().await?;
    let outcome = state
        .mailer
        .as_ref()
        .expect("mailer checked")
        .send_reply(&o)
        .await;
    let (status, code, retry) = match outcome {
        crate::notify::ReplyOutcome::Accepted => ("sent", None, false),
        crate::notify::ReplyOutcome::RejectedTransient => (
            if claim.attempt < 3 {
                "queued"
            } else {
                "failed"
            },
            Some("smtp_rejected_transient"),
            claim.attempt < 3,
        ),
        crate::notify::ReplyOutcome::RejectedPermanent => ("failed", Some("smtp_rejected"), false),
        crate::notify::ReplyOutcome::Unknown => {
            ("delivery_unknown", Some("smtp_result_unknown"), false)
        }
        crate::notify::ReplyOutcome::Invalid => ("failed", Some("invalid_message"), false),
    };
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    jobs::current(&mut tx, claim).await?;
    sqlx::query(
        "UPDATE outbound_replies SET state=?,error_code=?,sent_at=? WHERE id=? AND state='sending'",
    )
    .bind(status)
    .bind(code)
    .bind((status == "sent").then(db::now))
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE workflow_jobs SET state=?,error_code=?,available_at=?,token=NULL,lease_until=NULL WHERE id=? AND token=?").bind(if retry{"queued"}else if status=="sent"{"succeeded"}else if status=="delivery_unknown"{"delivery_unknown"}else{"failed"}).bind(code).bind(chrono::Utc::now().timestamp()+if claim.attempt==1{5}else{30}).bind(claim.id).bind(&claim.token).execute(&mut *tx).await?;
    workflow_audit::record(
        &mut tx,
        Some(o.site_id),
        o.actor_id,
        "outbound",
        id,
        status,
        json!({"message_id":o.message_id}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
