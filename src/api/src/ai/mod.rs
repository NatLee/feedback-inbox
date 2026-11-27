pub mod analysis;
pub mod auto_reply;
pub mod knowledge;
pub mod policy;
pub mod providers;
pub mod replies;
pub mod secrets;
use crate::error::{AppError, AppResult};
/// Re-read authority at each side-effect boundary, not just at request extraction.
pub async fn authority(
    tx: &mut sqlx::SqliteConnection,
    actor: i64,
    site: Option<i64>,
    admin: bool,
) -> AppResult<()> {
    let role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM users WHERE id=? AND disabled=0 AND account_status='active'",
    )
    .bind(actor)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(role) = role else {
        return Err(AppError::forbidden("執行者已停用或移除"));
    };
    if role == "admin" {
        return Ok(());
    }
    if admin {
        return Err(AppError::forbidden("需要管理員權限"));
    }
    if let Some(site) = site {
        let member: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM site_members WHERE site_id=? AND user_id=?)",
        )
        .bind(site)
        .bind(actor)
        .fetch_one(tx)
        .await?;
        if member {
            return Ok(());
        }
    }
    Err(AppError::forbidden("執行者已沒有網站權限"))
}
