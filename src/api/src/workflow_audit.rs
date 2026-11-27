use crate::{db, error::AppResult};
use serde_json::Value;
pub async fn record(
    tx: &mut sqlx::SqliteConnection,
    site: Option<i64>,
    actor: Option<i64>,
    entity: &str,
    id: i64,
    action: &str,
    metadata: Value,
) -> AppResult<()> {
    sqlx::query("INSERT INTO workflow_audit(site_id,actor_id,entity_type,entity_id,action,metadata_json,created_at) VALUES(?,?,?,?,?,?,?)").bind(site).bind(actor).bind(entity).bind(id.to_string()).bind(action).bind(metadata.to_string()).bind(db::now()).execute(tx).await?;
    Ok(())
}
