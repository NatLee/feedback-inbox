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
