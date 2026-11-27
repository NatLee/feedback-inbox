//! Durable claims. A claim token fences late workers after cancellation or lease recovery.
use crate::{
    auth, db,
    error::{AppError, AppResult},
    state::AppState,
};
use serde_json::Value;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

#[derive(Debug, Clone, FromRow)]
pub struct Claim {
    pub id: i64,
    pub site_id: i64,
    pub kind: String,
    pub payload_json: String,
    pub attempt: i64,
    pub token: String,
}
pub async fn enqueue(
    tx: &mut SqliteConnection,
    site_id: i64,
    kind: &str,
    payload: Value,
    key: &str,
) -> AppResult<i64> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query("INSERT OR IGNORE INTO workflow_jobs(site_id,kind,payload_json,unique_key,available_at,created_at) VALUES(?,?,?,?,?,?)").bind(site_id).bind(kind).bind(payload.to_string()).bind(key).bind(now).bind(db::now()).execute(&mut *tx).await?;
    Ok(
        sqlx::query_scalar("SELECT id FROM workflow_jobs WHERE unique_key=?")
            .bind(key)
            .fetch_one(tx)
            .await?,
    )
}
pub async fn claim(db: &SqlitePool, now: i64) -> AppResult<Option<Claim>> {
    claim_with_limit(db, now, 2).await
}
async fn claim_with_limit(
    db: &SqlitePool,
    now: i64,
    concurrency: usize,
) -> AppResult<Option<Claim>> {
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    // SMTP may already have accepted the message. Never reclaim an ambiguous send.
    sqlx::query("UPDATE outbound_replies SET state='delivery_unknown',error_code='worker_lost' WHERE state='sending' AND job_id IN (SELECT id FROM workflow_jobs WHERE state='running' AND lease_until<?)").bind(now).execute(&mut *tx).await?;
    // A worker that never crossed queued -> sending has not contacted SMTP and is safe to retry.
    sqlx::query("UPDATE outbound_replies SET state='failed',error_code='worker_lease_expired' WHERE state='queued' AND job_id IN (SELECT id FROM workflow_jobs WHERE kind='reply' AND state='running' AND lease_until<? AND attempt>=3)").bind(now).execute(&mut *tx).await?;
    sqlx::query("UPDATE ai_runs SET state=CASE WHEN (SELECT attempt FROM workflow_jobs WHERE id=ai_runs.job_id)>=3 THEN 'failed' ELSE 'queued' END,error_code='worker_lease_expired',error_message='工作租約到期，系統將依重試上限處理' WHERE state='running' AND job_id IN (SELECT id FROM workflow_jobs WHERE kind='analysis' AND state='running' AND lease_until<?)").bind(now).execute(&mut *tx).await?;
    sqlx::query("UPDATE workflow_jobs SET state=CASE WHEN kind='reply' AND EXISTS(SELECT 1 FROM outbound_replies o WHERE o.job_id=workflow_jobs.id AND o.state='queued') THEN 'queued' WHEN kind='reply' AND EXISTS(SELECT 1 FROM outbound_replies o WHERE o.job_id=workflow_jobs.id AND o.state='failed') THEN 'failed' WHEN kind IN ('reply','notify_email','notify_webhook') THEN 'delivery_unknown' WHEN attempt>=3 THEN 'failed' ELSE 'queued' END, token=NULL, lease_until=NULL, available_at=? WHERE state='running' AND lease_until<?").bind(now+5).bind(now).execute(&mut *tx).await?;
    let id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM workflow_jobs WHERE state='queued' AND available_at<=? AND (kind!='analysis' OR (SELECT COUNT(*) FROM workflow_jobs WHERE kind='analysis' AND state='running')<?) ORDER BY id LIMIT 1",
    )
    .bind(now)
    .bind(concurrency.clamp(1,8) as i64)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(id) = id else {
        tx.commit().await?;
        return Ok(None);
    };
    let token = auth::random_hex(16);
    sqlx::query("UPDATE workflow_jobs SET state='running',attempt=attempt+1,token=?,lease_until=? WHERE id=? AND state='queued'").bind(&token).bind(now+180).bind(id).execute(&mut *tx).await?;
    let result = sqlx::query_as::<_, Claim>(
        "SELECT id,site_id,kind,payload_json,attempt,token FROM workflow_jobs WHERE id=?",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some(result))
}
pub async fn current(tx: &mut SqliteConnection, claim: &Claim) -> AppResult<()> {
    let live:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_jobs WHERE id=? AND state='running' AND token=? AND lease_until>=?)").bind(claim.id).bind(&claim.token).bind(chrono::Utc::now().timestamp()).fetch_one(tx).await?;
    if !live {
        return Err(AppError::conflict("工作已取消或租約失效"));
    }
    Ok(())
}
pub async fn finish(
    db: &SqlitePool,
    claim: &Claim,
    state: &str,
    error: Option<&str>,
    retry_after: Option<i64>,
) -> AppResult<bool> {
    let retry = retry_after.filter(|_| claim.attempt < 3);
    let result=sqlx::query("UPDATE workflow_jobs SET state=?,error_code=?,available_at=?,token=NULL,lease_until=NULL WHERE id=? AND state='running' AND token=? AND lease_until>=?").bind(if retry.is_some(){"queued"}else{state}).bind(error).bind(chrono::Utc::now().timestamp()+retry.unwrap_or(0).clamp(0,300)).bind(claim.id).bind(&claim.token).bind(chrono::Utc::now().timestamp()).execute(db).await?;
    Ok(result.rows_affected() == 1)
}
pub async fn cancel(db: &SqlitePool, id: i64) -> AppResult<bool> {
    let result=sqlx::query("UPDATE workflow_jobs SET state='cancelled',token=NULL,lease_until=NULL WHERE id=? AND state IN ('queued','running') AND kind!='reply'").bind(id).execute(db).await?;
    Ok(result.rows_affected() == 1)
}
pub async fn heartbeat(db: &SqlitePool, claim: &Claim) -> AppResult<bool> {
    Ok(sqlx::query(
        "UPDATE workflow_jobs SET lease_until=? WHERE id=? AND state='running' AND token=? AND lease_until>=?",
    )
    .bind(chrono::Utc::now().timestamp() + 180)
    .bind(claim.id)
    .bind(&claim.token)
    .bind(chrono::Utc::now().timestamp())
    .execute(db)
    .await?
    .rows_affected()
        == 1)
}
pub async fn cleanup(db: &SqlitePool) -> AppResult<()> {
    let now = chrono::Utc::now().timestamp();
    let mut tx = db.begin().await?;
    for table in [
        "submission_receipts",
        "identity_nonces",
        "verification_sessions",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE expires_at<?"))
            .bind(now)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("DELETE FROM feedback WHERE is_test=1 AND created_at<strftime('%Y-%m-%dT%H:%M:%SZ','now','-1 day')")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
pub async fn run_once(state: &AppState) -> AppResult<bool> {
    let Some(claim) = claim_with_limit(
        &state.db,
        chrono::Utc::now().timestamp(),
        state.config.workflow.concurrency,
    )
    .await?
    else {
        return Ok(false);
    };
    let execute = async {
        match claim.kind.as_str() {
            "notify_email" | "notify_webhook" => crate::notify::execute(state, &claim).await,
            "analysis" => crate::ai::analysis::execute(state, &claim).await,
            "reply" => crate::ai::replies::execute(state, &claim).await,
            _ => Err(AppError::bad("未知工作類型")),
        }
    };
    tokio::pin!(execute);
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
    interval.tick().await;
    let result = loop {
        tokio::select! {result=&mut execute=>break result,_=interval.tick()=>{if !heartbeat(&state.db,&claim).await?{break Err(AppError::conflict("工作已取消"))}}}
    };
    if let Err(error) = result {
        let (_, _, code) = error.public_parts();
        let finished = finish(
            &state.db,
            &claim,
            "failed",
            Some(code.as_deref().unwrap_or("job_failed")),
            None,
        )
        .await?;
        if finished && claim.kind == "analysis" {
            sqlx::query("UPDATE ai_runs SET state='failed',error_code='job_failed',error_message='工作未完成，請檢查設定後重新執行' WHERE job_id=? AND state IN ('queued','running')").bind(claim.id).execute(&state.db).await?;
        }
    }
    Ok(true)
}
/// Explicit lifetime: callers signal shutdown and await every worker before closing SQLite.
pub async fn run_worker(state: AppState, mut shutdown: tokio::sync::watch::Receiver<bool>) {
    let mut workers = tokio::task::JoinSet::new();
    for _ in 0..state.config.workflow.concurrency {
        let state = state.clone();
        let mut stop = shutdown.clone();
        workers.spawn(async move{loop{if *stop.borrow(){break}match run_once(&state).await{Ok(true)=>continue,Err(_)=>tracing::warn!("workflow iteration failed"),Ok(false)=>{}}tokio::select!{_=tokio::time::sleep(std::time::Duration::from_secs(1))=>{},_=stop.changed()=>{if *stop.borrow(){break}}}}});
    }
    loop {
        tokio::select! {_=shutdown.changed()=>{if *shutdown.borrow(){break}},_=tokio::time::sleep(std::time::Duration::from_secs(60))=>{if cleanup(&state.db).await.is_err(){tracing::warn!("workflow cleanup failed")}}}
    }
    while workers.join_next().await.is_some() {}
}
