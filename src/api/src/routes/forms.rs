use super::sites::ensure_visible;
use crate::{
    auth::{Admin, CurrentUser},
    error::AppResult,
    forms::{self, FormDraft, FormSchema, FormVersion},
    state::AppState,
};
use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
#[derive(Deserialize)]
pub struct DraftBody {
    expected_revision: i64,
    schema: FormSchema,
}
#[derive(Deserialize)]
pub struct RevisionBody {
    expected_revision: i64,
}
pub async fn detail(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    ensure_visible(&s, &u, id).await?;
    Ok(Json(forms::detail(&s.db, id, u.is_admin()).await?))
}
pub async fn draft(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path(id): Path<i64>,
    Json(b): Json<DraftBody>,
) -> AppResult<Json<FormDraft>> {
    Ok(Json(
        forms::save_draft(&s.db, id, b.expected_revision, b.schema, u.id).await?,
    ))
}
pub async fn publish(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path(id): Path<i64>,
    Json(b): Json<RevisionBody>,
) -> AppResult<Json<FormVersion>> {
    Ok(Json(
        forms::publish(&s.db, id, b.expected_revision, u.id).await?,
    ))
}
pub async fn retire(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path((id, version)): Path<(i64, i64)>,
) -> AppResult<Json<Value>> {
    forms::retire(&s.db, id, version, u.id).await?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
pub struct IntegrationQuery {
    format: String,
}
pub async fn integrations(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<i64>,
    axum::extract::Query(q): axum::extract::Query<IntegrationQuery>,
) -> AppResult<Json<Value>> {
    ensure_visible(&s, &u, id).await?;
    let version = forms::load_version(&s.db, id, None).await?;
    let key: String = sqlx::query_scalar("SELECT key FROM sites WHERE id=?")
        .bind(id)
        .fetch_one(&s.db)
        .await?;
    let base = s
        .config
        .public_url
        .as_deref()
        .unwrap_or("http://127.0.0.1:8787");
    let endpoint = format!("{base}/f/{key}");
    Ok(Json(
        json!({"format":q.format,"version":version.version,"endpoint":endpoint,"code":forms::snippets::generate(&q.format,&version,&endpoint)?}),
    ))
}

pub async fn identity_info(
    State(s): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let configured: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM identity_keys WHERE site_id=?)")
            .bind(id)
            .fetch_one(&s.db)
            .await?;
    Ok(Json(
        json!({"configured":configured,"encryption_ready":s.config.workflow.secret_key.is_some(),"example":forms::identity::node_example(id)}),
    ))
}
pub async fn rotate_identity(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    forms::ensure_site(&s.db, id).await?;
    Ok(Json(forms::identity::rotate(&s, id, u.id).await?))
}
pub async fn create_verification(
    State(s): State<AppState>,
    Admin(u): Admin,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    forms::ensure_site(&s.db, id).await?;
    Ok(Json(
        forms::identity::create_verification(&s, id, u.id).await?,
    ))
}
pub async fn verification_status(
    State(s): State<AppState>,
    u: CurrentUser,
    Path((id, session)): Path<(i64, i64)>,
) -> AppResult<Json<Value>> {
    ensure_visible(&s, &u, id).await?;
    let row:Option<(i64,i64,Option<String>)>=sqlx::query_as("SELECT expires_at,received,last_received_at FROM verification_sessions WHERE id=? AND site_id=?").bind(session).bind(id).fetch_optional(&s.db).await?;
    let (expires_at, received, last_received_at) =
        row.ok_or_else(|| crate::error::AppError::not_found("找不到安裝測試"))?;
    Ok(Json(
        json!({"id":session,"expires_at":expires_at,"received":received,"last_received_at":last_received_at}),
    ))
}
