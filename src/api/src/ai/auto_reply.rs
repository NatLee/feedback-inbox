//! Automatic replies are approved fixed templates behind deterministic gates, never model prose.
use super::{analysis, policy, replies};
use crate::{
    auth, db,
    error::{AppError, AppResult},
    forms::{FieldKind, FormSchema},
    jobs,
    state::AppState,
    workflow_audit,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;
#[derive(Deserialize, Serialize, FromRow, Clone)]
pub struct Rule {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub field_id: String,
    #[sqlx(rename = "equals_value")]
    pub equals: String,
    pub category: String,
    pub template_id: i64,
}
pub async fn list(state: &AppState, site: i64) -> AppResult<Value> {
    let items: Vec<Rule> =
        sqlx::query_as("SELECT * FROM ai_rules WHERE site_id=? AND deleted=0 ORDER BY id")
            .bind(site)
            .fetch_all(&state.db)
            .await?;
    Ok(json!({"items":items}))
}
pub async fn save(
    state: &AppState,
    site: i64,
    actor: i64,
    id: Option<i64>,
    b: Rule,
) -> AppResult<Rule> {
    if b.name.trim().is_empty() || b.name.chars().count() > 100 {
        return Err(AppError::bad("規則名稱需為 1–100 字"));
    }
    let form = crate::forms::load_version(&state.db, site, None).await?;
    let field = form
        .schema
        .fields
        .iter()
        .find(|f| f.id == b.field_id && f.kind == FieldKind::Select)
        .ok_or_else(|| AppError::bad("規則必須選取已發布的選單欄位"))?;
    if !field.options.iter().any(|o| o.value == b.equals) {
        return Err(AppError::bad("規則值必須是已發布選項的精確值"));
    }
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    super::authority(&mut tx, actor, Some(site), true).await?;
    let settings = policy::load(&mut tx, site).await?;
    if !settings.categories.contains(&b.category) {
        return Err(AppError::bad("規則分類不在允許分類內"));
    }
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM ai_templates WHERE id=? AND site_id=? AND state='approved')",
    )
    .bind(b.template_id)
    .bind(site)
    .fetch_one(&mut *tx)
    .await?;
    if !valid {
        return Err(AppError::bad("請選擇已核准範本版本"));
    }
    let id = if let Some(id) = id {
        let changed=sqlx::query("UPDATE ai_rules SET name=?,enabled=?,field_id=?,equals_value=?,category=?,template_id=? WHERE id=? AND site_id=? AND deleted=0").bind(&b.name).bind(b.enabled).bind(&b.field_id).bind(&b.equals).bind(&b.category).bind(b.template_id).bind(id).bind(site).execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            return Err(AppError::not_found("找不到規則"));
        }
        id
    } else {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM ai_rules WHERE site_id=? AND deleted=0")
                .bind(site)
                .fetch_one(&mut *tx)
                .await?;
        if count >= 20 {
            return Err(AppError::bad("每個網站最多 20 條自動回覆規則"));
        }
        sqlx::query("INSERT INTO ai_rules(site_id,name,enabled,field_id,equals_value,category,template_id) VALUES(?,?,?,?,?,?,?)").bind(site).bind(&b.name).bind(b.enabled).bind(&b.field_id).bind(&b.equals).bind(&b.category).bind(b.template_id).execute(&mut *tx).await?.last_insert_rowid()
    };
    policy::touch(&mut tx, site).await?;
    workflow_audit::record(
        &mut tx,
        Some(site),
        Some(actor),
        "auto_rule",
        id,
        "saved",
        json!({"enabled":b.enabled,"template_id":b.template_id}),
    )
    .await?;
    tx.commit().await?;
    Ok(Rule { id, ..b })
}
pub async fn remove(state: &AppState, site: i64, actor: i64, id: i64) -> AppResult<()> {
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    super::authority(&mut tx, actor, Some(site), true).await?;
    let result = sqlx::query(
        "UPDATE ai_rules SET deleted=1,enabled=0 WHERE id=? AND site_id=? AND deleted=0",
    )
    .bind(id)
    .bind(site)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::not_found("找不到規則"));
    }
    policy::touch(&mut tx, site).await?;
    workflow_audit::record(
        &mut tx,
        Some(site),
        Some(actor),
        "auto_rule",
        id,
        "removed",
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
#[derive(Serialize)]
pub struct Decision {
    pub eligible: bool,
    pub gate: String,
    pub explanation: String,
    #[serde(skip)]
    pub(crate) approved: Option<Eligible>,
}
pub struct Eligible {
    pub site: i64,
    pub feedback: i64,
    pub actor: i64,
    pub policy_revision: i64,
    pub provider_revision: i64,
    pub rule: i64,
    pub template: i64,
    pub source_hash: String,
    pub recipient: String,
    pub subject: String,
    pub body: String,
    pub knowledge_ids: Vec<i64>,
    pub daily_limit: i64,
}
fn review(gate: &str, explanation: &str) -> Decision {
    Decision {
        eligible: false,
        gate: gate.into(),
        explanation: explanation.into(),
        approved: None,
    }
}
pub async fn evaluate(
    tx: &mut sqlx::SqliteConnection,
    state: &AppState,
    feedback: i64,
    reserved: Option<&replies::Outbound>,
) -> AppResult<Decision> {
    use sqlx::Row;
    let source = analysis::source(tx, feedback).await?;
    let settings = policy::load(tx, source.site_id).await?;
    if !state.config.workflow.auto_send_enabled {
        return Ok(review(
            "deployment_disabled",
            "部署自動寄送開關未啟用；請人工審核",
        ));
    }
    if state.mailer.is_none() {
        return Ok(review("smtp_missing", "尚未設定 SMTP"));
    }
    if settings.auto_mode != "auto_template" || !settings.enabled {
        return Ok(review("policy_off", "網站自動回覆已暫停"));
    }
    let Some(actor) = settings.updated_by else {
        return Ok(review("authority_changed", "自動政策核准者已移除"));
    };
    if super::authority(tx, actor, Some(source.site_id), true)
        .await
        .is_err()
    {
        return Ok(review(
            "authority_changed",
            "自動政策核准者已停用或失去管理權限",
        ));
    }
    if source.id <= settings.activation_feedback_id.unwrap_or(i64::MAX) {
        return Ok(review("before_activation", "啟用前的歷史訊息需要人工審核"));
    }
    if source.is_test || source.status != "new" {
        return Ok(review("not_new", "只有全新的正式訊息可以自動回覆"));
    }
    let created = chrono::DateTime::parse_from_rfc3339(&source.created_at)
        .map_err(|_| AppError::bad("訊息時間格式不符"))?
        .timestamp();
    let now = chrono::Utc::now().timestamp();
    if now - created > 86400 || created > now + 60 {
        return Ok(review("age", "訊息已超過 24 小時或時間無效"));
    }
    let identity: Value = serde_json::from_str(&source.identity_json)?;
    if identity["email_verified"] != true
        || identity["email"]
            .as_str()
            .is_none_or(|email| !email.eq_ignore_ascii_case(&source.email))
    {
        return Ok(review(
            "recipient_unverified",
            "需要網站後端簽署並驗證 Email 的會員身分；公開填寫的 Email 不能自動寄送",
        ));
    }
    let Some(version) = source.form_version_id else {
        return Ok(review("legacy_source", "歷史未版本化訊息需人工審核"));
    };
    let schema: Option<String> = sqlx::query_scalar(
        "SELECT schema_json FROM form_versions WHERE id=? AND site_id=? AND retired_at IS NULL",
    )
    .bind(version)
    .bind(source.site_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(schema) = schema else {
        return Ok(review("source_retired", "來源表單版本已停用"));
    };
    let schema: FormSchema = serde_json::from_str(&schema)?;
    let answers: Value = serde_json::from_str(&source.answers_json)?;
    if !schema.fields.iter().any(|f| {
        f.key == "reply_requested" && f.kind == FieldKind::Checkbox && answers[&f.id] == true
    }) {
        return Ok(review("consent_missing", "訪客尚未明確勾選希望收到回覆"));
    }
    let previous: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM outbound_replies WHERE feedback_id=? AND id!=?)",
    )
    .bind(feedback)
    .bind(reserved.map(|o| o.id).unwrap_or(-1))
    .fetch_one(&mut *tx)
    .await?;
    if previous {
        return Ok(review("previous_reply", "此訊息已有回覆紀錄，請人工確認"));
    }
    let manual_review:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM reply_drafts WHERE feedback_id=? AND (origin='human' OR state='rejected'))").bind(feedback).fetch_one(&mut *tx).await?;
    if manual_review {
        return Ok(review(
            "human_review_in_progress",
            "這封來信已有人工撰寫或退回的草稿，請完成原本的人工審核",
        ));
    }
    let run: Option<analysis::Run> = sqlx::query_as(
        "SELECT * FROM ai_runs WHERE feedback_id=? AND state='succeeded' ORDER BY id DESC LIMIT 1",
    )
    .bind(feedback)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(run) = run else {
        return Ok(review("analysis_missing", "尚無通過證據驗證的分析結果"));
    };
    if analysis::recheck(tx, state, &run).await.is_err() {
        return Ok(review(
            "analysis_stale",
            "分析權限、來源、供應商或政策版本已變更",
        ));
    }
    let output: String = sqlx::query_scalar("SELECT output_json FROM ai_runs WHERE id=?")
        .bind(run.id)
        .fetch_one(&mut *tx)
        .await?;
    let output: analysis::Analysis = serde_json::from_str(&output)?;
    if !output.risk_flags.is_empty()
        || !output.missing_information.is_empty()
        || output.urgency == "high"
    {
        return Ok(review(
            "risk_or_missing_information",
            "分析指出風險、高優先事項或資訊不足，需人工審核",
        ));
    }
    let lowered = source.message.to_lowercase();
    if [
        "refund",
        "password",
        "breach",
        "legal",
        "lawsuit",
        "suicide",
        "credit card",
        "退款",
        "密碼",
        "個資",
        "信用卡",
        "自殺",
        "訴訟",
        "駭客",
    ]
    .iter()
    .any(|term| lowered.contains(term))
    {
        return Ok(review("sensitive_topic", "訊息包含需人工審核的敏感主題"));
    }
    let rules: Vec<Rule> =
        sqlx::query_as("SELECT * FROM ai_rules WHERE site_id=? AND enabled=1 AND deleted=0")
            .bind(source.site_id)
            .fetch_all(&mut *tx)
            .await?;
    let matches: Vec<_> = rules
        .into_iter()
        .filter(|r| {
            r.category == output.category
                && answers[&r.field_id].as_str() == Some(r.equals.as_str())
                && schema.fields.iter().any(|f| {
                    f.id == r.field_id
                        && f.kind == FieldKind::Select
                        && f.options.iter().any(|o| o.value == r.equals)
                })
        })
        .collect();
    if matches.len() != 1 {
        return Ok(review(
            if matches.is_empty() {
                "no_rule"
            } else {
                "ambiguous_rules"
            },
            "必須且只能符合一條明確的選單與分類規則",
        ));
    }
    let rule = &matches[0];
    let template=sqlx::query("SELECT t.* FROM ai_templates t JOIN users u ON u.id=t.approved_by WHERE t.id=? AND t.site_id=? AND t.state='approved' AND u.role='admin' AND u.disabled=0").bind(rule.template_id).bind(source.site_id).fetch_optional(&mut *tx).await?;
    let Some(template) = template else {
        return Ok(review("template_unapproved", "範本版本已變更或尚未核准"));
    };
    let knowledge: Vec<i64> = serde_json::from_str(template.get::<&str, _>("knowledge_ids_json"))?;
    if policy::knowledge_valid(tx, source.site_id, &knowledge)
        .await
        .is_err()
    {
        return Ok(review("knowledge_stale", "範本引用知識已失效"));
    }
    if let Some(reserved) = reserved {
        if reserved.rule_id != Some(rule.id)
            || reserved.template_id != Some(rule.template_id)
            || reserved.policy_revision != Some(settings.revision)
            || reserved.provider_revision != Some(run.provider_revision)
        {
            return Ok(review(
                "reservation_stale",
                "已保留寄送的規則或政策版本已變更",
            ));
        }
    } else {
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let count: Option<i64> =
            sqlx::query_scalar("SELECT sends FROM auto_daily_quota WHERE site_id=? AND day=?")
                .bind(source.site_id)
                .bind(day)
                .fetch_optional(&mut *tx)
                .await?;
        if count.unwrap_or(0) >= settings.auto_daily_limit {
            return Ok(review("site_quota", "已達網站今日 UTC 自動回覆上限"));
        }
        let last: Option<i64> = sqlx::query_scalar(
            "SELECT reserved_at FROM auto_recipient_quota WHERE site_id=? AND recipient_hash=?",
        )
        .bind(source.site_id)
        .bind(auth::sha256_hex(&source.email.to_lowercase()))
        .fetch_optional(&mut *tx)
        .await?;
        if last.is_some_and(|at| at > now - 86400) {
            return Ok(review(
                "recipient_quota",
                "同一收件者 24 小時內已有自動回覆保留紀錄",
            ));
        }
    }
    let name: String = sqlx::query_scalar("SELECT name FROM sites WHERE id=?")
        .bind(source.site_id)
        .fetch_one(&mut *tx)
        .await?;
    let render = |value: &str| {
        value
            .replace("{site_name}", &name)
            .replace("{reference}", &feedback.to_string())
            .replace("{category}", &output.category)
    };
    let subject = render(template.get("subject"));
    let body = render(template.get("body"));
    if replies::validate_content(&source.email, &subject, &body).is_err() {
        return Ok(review(
            "render_invalid",
            "範本產生的內容或收件地址不符合限制",
        ));
    }
    Ok(Decision {
        eligible: true,
        gate: "eligible".into(),
        explanation: "已符合核准範本、身分、明確回覆意願、分類與額度規則".into(),
        approved: Some(Eligible {
            site: source.site_id,
            feedback,
            actor,
            policy_revision: settings.revision,
            provider_revision: run.provider_revision,
            rule: rule.id,
            template: rule.template_id,
            source_hash: analysis::source_hash(&source),
            recipient: source.email,
            subject,
            body,
            knowledge_ids: knowledge,
            daily_limit: settings.auto_daily_limit,
        }),
    })
}
pub async fn reserve(
    tx: &mut sqlx::SqliteConnection,
    state: &AppState,
    feedback: i64,
) -> AppResult<()> {
    let decision = evaluate(tx, state, feedback, None).await?;
    sqlx::query("INSERT INTO auto_reply_decisions(feedback_id,gate,explanation,created_at) VALUES(?,?,?,?) ON CONFLICT(feedback_id) DO UPDATE SET gate=excluded.gate,explanation=excluded.explanation,created_at=excluded.created_at").bind(feedback).bind(&decision.gate).bind(&decision.explanation).bind(db::now()).execute(&mut *tx).await?;
    let Some(e) = decision.approved else {
        return Ok(());
    };
    let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
    sqlx::query("INSERT OR IGNORE INTO auto_daily_quota(site_id,day,sends) VALUES(?,?,0)")
        .bind(e.site)
        .bind(&day)
        .execute(&mut *tx)
        .await?;
    let reserved = sqlx::query(
        "UPDATE auto_daily_quota SET sends=sends+1 WHERE site_id=? AND day=? AND sends<?",
    )
    .bind(e.site)
    .bind(day)
    .bind(e.daily_limit)
    .execute(&mut *tx)
    .await?;
    if reserved.rows_affected() != 1 {
        return Err(AppError::conflict("自動回覆額度已用完"));
    }
    sqlx::query("INSERT INTO auto_recipient_quota(site_id,recipient_hash,reserved_at) VALUES(?,?,?) ON CONFLICT(site_id,recipient_hash) DO UPDATE SET reserved_at=excluded.reserved_at").bind(e.site).bind(auth::sha256_hex(&e.recipient.to_lowercase())).bind(chrono::Utc::now().timestamp()).execute(&mut *tx).await?;
    let hash = replies::content_hash(&e.recipient, &e.subject, &e.body, &e.knowledge_ids);
    let message_id = format!("<fi-{}@feedback-inbox.invalid>", auth::random_hex(16));
    let id=sqlx::query("INSERT INTO outbound_replies(feedback_id,site_id,mode,template_id,rule_id,actor_id,policy_revision,provider_revision,source_hash,recipient,subject,body,knowledge_ids_json,content_hash,message_id,idempotency_key,request_hash,created_at) VALUES(?,?,'auto_template',?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)").bind(e.feedback).bind(e.site).bind(e.template).bind(e.rule).bind(e.actor).bind(e.policy_revision).bind(e.provider_revision).bind(&e.source_hash).bind(&e.recipient).bind(&e.subject).bind(&e.body).bind(serde_json::to_string(&e.knowledge_ids)?).bind(&hash).bind(message_id).bind(format!("auto:{feedback}")).bind(&hash).bind(db::now()).execute(&mut *tx).await?.last_insert_rowid();
    let job = jobs::enqueue(
        tx,
        e.site,
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
    sqlx::query("UPDATE reply_drafts SET state='superseded' WHERE feedback_id=? AND origin='ai' AND state='draft'").bind(feedback).execute(&mut *tx).await?;
    workflow_audit::record(tx,Some(e.site),Some(e.actor),"outbound",id,"auto_reserved",json!({"rule_id":e.rule,"template_id":e.template,"policy_revision":e.policy_revision,"content_hash":hash})).await?;
    Ok(())
}
