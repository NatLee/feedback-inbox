pub mod identity;
pub mod render;
pub mod snippets;
pub mod types;
pub mod validation;
use crate::{
    auth, db,
    error::{AppError, AppResult},
    routes::sites::parse_list,
};
use serde_json::{json, Value};
use sqlx::{SqliteConnection, SqlitePool};
pub use types::*;
pub use validation::*;

#[derive(sqlx::FromRow)]
struct VersionRow {
    id: i64,
    site_id: i64,
    version: i64,
    schema_json: String,
    schema_hash: String,
    published_at: String,
    retired_at: Option<String>,
}
impl VersionRow {
    fn present(self) -> AppResult<FormVersion> {
        Ok(FormVersion {
            id: self.id,
            site_id: self.site_id,
            version: self.version,
            schema: serde_json::from_str(&self.schema_json)?,
            schema_hash: self.schema_hash,
            published_at: self.published_at,
            retired_at: self.retired_at,
        })
    }
}
pub async fn backfill(pool: &SqlitePool) -> AppResult<()> {
    let ids: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM sites WHERE published_form_version_id IS NULL")
            .fetch_all(pool)
            .await?;
    for id in ids {
        ensure_site(pool, id).await?;
    }
    Ok(())
}
pub async fn ensure_site(pool: &SqlitePool, site_id: i64) -> AppResult<()> {
    let ready:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sites s JOIN form_drafts d ON d.site_id=s.id WHERE s.id=? AND s.published_form_version_id IS NOT NULL)").bind(site_id).fetch_one(pool).await?;
    if ready {
        return Ok(());
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let (name, required): (String, String) =
        sqlx::query_as("SELECT name,required_fields FROM sites WHERE id=?")
            .bind(site_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| AppError::not_found("找不到網站"))?;
    let schema = serde_json::to_string(&FormSchema::legacy(&name, &parse_list(&required)))?;
    sqlx::query("INSERT OR IGNORE INTO form_versions(site_id,version,schema_json,schema_hash,published_at) VALUES (?,1,?,?,?)").bind(site_id).bind(&schema).bind(auth::sha256_hex(&schema)).bind(db::now()).execute(&mut *tx).await?;
    sqlx::query("UPDATE sites SET published_form_version_id=(SELECT id FROM form_versions WHERE site_id=? AND version=1) WHERE id=? AND published_form_version_id IS NULL").bind(site_id).bind(site_id).execute(&mut *tx).await?;
    sqlx::query("INSERT OR IGNORE INTO form_drafts(site_id,schema_json,updated_at) SELECT ?,schema_json,? FROM form_versions WHERE id=(SELECT published_form_version_id FROM sites WHERE id=?)").bind(site_id).bind(db::now()).bind(site_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn load_version(
    pool: &SqlitePool,
    site_id: i64,
    requested: Option<i64>,
) -> AppResult<FormVersion> {
    ensure_site(pool, site_id).await?;
    let row: Option<VersionRow> = if let Some(v) = requested {
        sqlx::query_as("SELECT * FROM form_versions WHERE site_id=? AND version=?")
            .bind(site_id)
            .bind(v)
            .fetch_optional(pool)
            .await?
    } else {
        sqlx::query_as("SELECT v.* FROM form_versions v JOIN sites s ON s.published_form_version_id=v.id WHERE s.id=?").bind(site_id).fetch_optional(pool).await?
    };
    let version = row
        .ok_or_else(|| AppError::not_found("找不到表單版本"))?
        .present()?;
    if version.retired_at.is_some() {
        return Err(AppError::coded(
            410,
            "form_version_retired",
            "這個表單版本已停用，請重新開啟表單網址",
        ));
    }
    Ok(version)
}
pub async fn detail(pool: &SqlitePool, site_id: i64, admin: bool) -> AppResult<Value> {
    let published = load_version(pool, site_id, None).await?;
    let mut out = json!({"published":published});
    if admin {
        let (revision, schema): (i64, String) =
            sqlx::query_as("SELECT revision,schema_json FROM form_drafts WHERE site_id=?")
                .bind(site_id)
                .fetch_one(pool)
                .await?;
        out["draft"] = serde_json::to_value(FormDraft {
            revision,
            schema: serde_json::from_str(&schema)?,
        })?;
        let versions: Vec<VersionRow> =
            sqlx::query_as("SELECT * FROM form_versions WHERE site_id=? ORDER BY version DESC")
                .bind(site_id)
                .fetch_all(pool)
                .await?;
        out["versions"] = serde_json::to_value(
            versions
                .into_iter()
                .map(VersionRow::present)
                .collect::<AppResult<Vec<_>>>()?,
        )?;
    }
    Ok(out)
}
pub async fn save_draft(
    pool: &SqlitePool,
    site_id: i64,
    revision: i64,
    schema: FormSchema,
    user_id: i64,
) -> AppResult<FormDraft> {
    let previous = load_version(pool, site_id, None).await?;
    validate_schema(&schema, Some(&previous.schema))?;
    let result=sqlx::query("UPDATE form_drafts SET schema_json=?,revision=revision+1,updated_by=?,updated_at=? WHERE site_id=? AND revision=?").bind(serde_json::to_string(&schema)?).bind(user_id).bind(db::now()).bind(site_id).bind(revision).execute(pool).await?;
    if result.rows_affected() == 0 {
        return Err(AppError::conflict("草稿已由其他人更新，請重新載入"));
    }
    Ok(FormDraft {
        revision: revision + 1,
        schema,
    })
}
pub async fn publish(
    pool: &SqlitePool,
    site_id: i64,
    revision: i64,
    user_id: i64,
) -> AppResult<FormVersion> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let (actual, schema): (i64, String) =
        sqlx::query_as("SELECT revision,schema_json FROM form_drafts WHERE site_id=?")
            .bind(site_id)
            .fetch_one(&mut *tx)
            .await?;
    if actual != revision {
        return Err(AppError::conflict("草稿已更新，請重新載入"));
    }
    publish_schema(&mut tx, site_id, serde_json::from_str(&schema)?, user_id).await?;
    tx.commit().await?;
    load_version(pool, site_id, None).await
}
pub async fn publish_schema(
    tx: &mut SqliteConnection,
    site_id: i64,
    schema: FormSchema,
    user_id: i64,
) -> AppResult<()> {
    let previous: Vec<String> =
        sqlx::query_scalar("SELECT schema_json FROM form_versions WHERE site_id=?")
            .bind(site_id)
            .fetch_all(&mut *tx)
            .await?;
    validate_schema(&schema, None)?;
    for s in previous {
        validate_schema(&schema, Some(&serde_json::from_str(&s)?))?;
    }
    let version: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(version),0)+1 FROM form_versions WHERE site_id=?")
            .bind(site_id)
            .fetch_one(&mut *tx)
            .await?;
    let data = serde_json::to_string(&schema)?;
    let hash = auth::sha256_hex(&data);
    let id=sqlx::query("INSERT INTO form_versions(site_id,version,schema_json,schema_hash,published_by,published_at) VALUES(?,?,?,?,?,?)").bind(site_id).bind(version).bind(&data).bind(&hash).bind(user_id).bind(db::now()).execute(&mut *tx).await?.last_insert_rowid();
    let required: Vec<&str> = schema
        .fields
        .iter()
        .filter(|f| f.required && crate::intake::FIELDS.contains(&f.key.as_str()))
        .map(|f| f.key.as_str())
        .collect();
    sqlx::query("UPDATE sites SET published_form_version_id=?,required_fields=? WHERE id=?")
        .bind(id)
        .bind(serde_json::to_string(&required)?)
        .bind(site_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO workflow_audit(site_id,actor_id,entity_type,entity_id,action,metadata_json,created_at) VALUES(?,?,'form',?,'published',?,?)").bind(site_id).bind(user_id).bind(id.to_string()).bind(json!({"version":version,"hash":hash}).to_string()).bind(db::now()).execute(&mut *tx).await?;
    Ok(())
}
pub async fn publish_required(
    tx: &mut SqliteConnection,
    site_id: i64,
    required: &[String],
    user_id: i64,
) -> AppResult<()> {
    let s:String=sqlx::query_scalar("SELECT v.schema_json FROM form_versions v JOIN sites s ON s.published_form_version_id=v.id WHERE s.id=?").bind(site_id).fetch_one(&mut *tx).await?;
    let mut schema: FormSchema = serde_json::from_str(&s)?;
    for f in &mut schema.fields {
        if crate::intake::FIELDS.contains(&f.key.as_str()) {
            f.required = required.contains(&f.key);
        }
    }
    for key in required {
        if !schema.fields.iter().any(|f| &f.key == key) {
            return Err(AppError::bad(
                "必填基本欄位已從表單移除，請先到表單編輯器新增",
            ));
        }
    }
    publish_schema(tx, site_id, schema.clone(), user_id).await?;
    sqlx::query("UPDATE form_drafts SET schema_json=?,revision=revision+1,updated_by=?,updated_at=? WHERE site_id=?").bind(serde_json::to_string(&schema)?).bind(user_id).bind(db::now()).bind(site_id).execute(&mut *tx).await?;
    Ok(())
}
pub async fn retire(pool: &SqlitePool, site_id: i64, version: i64, user_id: i64) -> AppResult<()> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let active:Option<i64>=sqlx::query_scalar("SELECT v.version FROM sites s JOIN form_versions v ON s.published_form_version_id=v.id WHERE s.id=?").bind(site_id).fetch_optional(&mut *tx).await?;
    if active == Some(version) {
        return Err(AppError::conflict("目前發布中的版本不能停用"));
    }
    let done=sqlx::query("UPDATE form_versions SET retired_at=? WHERE site_id=? AND version=? AND retired_at IS NULL").bind(db::now()).bind(site_id).bind(version).execute(&mut *tx).await?;
    if done.rows_affected() == 0 {
        return Err(AppError::not_found("找不到可停用的版本"));
    }
    sqlx::query("INSERT INTO workflow_audit(site_id,actor_id,entity_type,entity_id,action,created_at) VALUES(?,?,'form',?,'retired',?)").bind(site_id).bind(user_id).bind(version.to_string()).bind(db::now()).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
