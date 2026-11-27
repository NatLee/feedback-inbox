use super::secrets;
use crate::{
    db,
    error::{AppError, AppResult},
    state::AppState,
    workflow_audit,
};
use ai_kit::{AiClient, CompletionRequest, ProviderConfig, ProviderKind, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::json;
#[derive(sqlx::FromRow, Clone)]
pub struct StoredProvider {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub model: String,
    pub enabled: bool,
    pub revision: i64,
    pub credential_ciphertext: Option<String>,
    pub tested_revision: Option<i64>,
    pub last_test_error: Option<String>,
}
#[derive(Deserialize)]
pub struct ProviderBody {
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub model: String,
    pub enabled: bool,
    pub credential: Option<String>,
    pub expected_revision: Option<i64>,
}
#[derive(Serialize)]
pub struct Provider {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub model: String,
    pub enabled: bool,
    pub revision: i64,
    pub has_credential: bool,
    pub tested_revision: Option<i64>,
    pub last_test_error: Option<String>,
}
impl StoredProvider {
    pub fn public(self) -> Provider {
        Provider {
            id: self.id,
            name: self.name,
            kind: self.kind,
            base_url: self.base_url,
            model: self.model,
            enabled: self.enabled,
            revision: self.revision,
            has_credential: self.credential_ciphertext.is_some(),
            tested_revision: self.tested_revision,
            last_test_error: self.last_test_error,
        }
    }
}
pub async fn load(tx: &mut sqlx::SqliteConnection, id: i64) -> AppResult<StoredProvider> {
    sqlx::query_as("SELECT * FROM ai_providers WHERE id=?")
        .bind(id)
        .fetch_optional(tx)
        .await?
        .ok_or_else(|| AppError::not_found("找不到 AI 供應商"))
}
pub async fn list(state: &AppState) -> AppResult<Vec<Provider>> {
    Ok(
        sqlx::query_as::<_, StoredProvider>("SELECT * FROM ai_providers ORDER BY id")
            .fetch_all(&state.db)
            .await?
            .into_iter()
            .map(StoredProvider::public)
            .collect(),
    )
}
pub fn destination(state: &AppState, kind: ProviderKind, raw: &str) -> AppResult<reqwest::Url> {
    let url = reqwest::Url::parse(raw).map_err(|_| AppError::bad("供應商網址格式不符"))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(AppError::bad(
            "供應商只接受根網址，不可有帳密、路徑或查詢參數",
        ));
    }
    let origin = url.origin().ascii_serialization();
    let allowed = match kind {
        ProviderKind::OpenAi => origin == "https://api.openai.com",
        ProviderKind::Anthropic => origin == "https://api.anthropic.com",
        ProviderKind::Ollama => {
            matches!(url.scheme(), "http" | "https")
                && state
                    .config
                    .workflow
                    .allowed_ollama_origins
                    .contains(&origin)
        }
    };
    if !allowed {
        return Err(AppError::bad("供應商網址不在部署允許清單內"));
    }
    Ok(url)
}
pub async fn save(
    state: &AppState,
    actor: i64,
    id: Option<i64>,
    body: ProviderBody,
) -> AppResult<Provider> {
    if body.name.trim().is_empty()
        || body.name.chars().count() > 80
        || body.model.trim().is_empty()
        || body.model.len() > 200
        || body.model.chars().any(char::is_control)
    {
        return Err(AppError::bad("名稱或模型格式不符"));
    }
    let url = destination(state, body.kind, &body.base_url)?;
    let kind = serde_json::to_value(body.kind)?
        .as_str()
        .unwrap()
        .to_owned();
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    super::authority(&mut tx, actor, None, true).await?;
    let id = if let Some(id) = id {
        let old = load(&mut tx, id).await?;
        if Some(old.revision) != body.expected_revision {
            return Err(AppError::conflict("供應商已更新，請重新載入"));
        }
        if old.kind != kind {
            return Err(AppError::bad("供應商種類不可變更，請新增設定"));
        }
        sqlx::query("UPDATE ai_providers SET name=?,base_url=?,model=?,enabled=?,revision=revision+1,tested_revision=NULL,last_test_error=NULL WHERE id=?").bind(body.name.trim()).bind(url.as_str()).bind(body.model.trim()).bind(body.enabled).bind(id).execute(&mut *tx).await?;
        id
    } else {
        sqlx::query("INSERT INTO ai_providers(name,kind,base_url,model,enabled,created_at) VALUES(?,?,?,?,?,?)").bind(body.name.trim()).bind(&kind).bind(url.as_str()).bind(body.model.trim()).bind(body.enabled).bind(db::now()).execute(&mut *tx).await?.last_insert_rowid()
    };
    if let Some(credential) = body.credential {
        let encrypted = if credential.is_empty() {
            None
        } else {
            Some(secrets::for_state(state)?.encrypt("provider", id, &credential)?)
        };
        sqlx::query("UPDATE ai_providers SET credential_ciphertext=? WHERE id=?")
            .bind(encrypted)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    workflow_audit::record(
        &mut tx,
        None,
        Some(actor),
        "provider",
        id,
        "saved",
        json!({"kind":kind}),
    )
    .await?;
    let result = load(&mut tx, id).await?.public();
    tx.commit().await?;
    Ok(result)
}
pub fn client(state: &AppState, provider: &StoredProvider) -> AppResult<AiClient> {
    let kind: ProviderKind = serde_json::from_value(json!(provider.kind))
        .map_err(|_| AppError::bad("不支援的供應商"))?;
    let base_url = destination(state, kind, &provider.base_url)?;
    let credential = provider
        .credential_ciphertext
        .as_ref()
        .map(|c| secrets::for_state(state)?.decrypt("provider", provider.id, c))
        .transpose()?
        .map(SecretString::new);
    AiClient::new(
        ProviderConfig {
            kind,
            base_url,
            model: provider.model.clone(),
        },
        credential,
    )
    .map_err(|e| AppError::bad(e.safe_message))
}
pub async fn test(state: &AppState, actor: i64, id: i64, revision: i64) -> AppResult<Provider> {
    let _permit = state
        .ai_slots
        .acquire()
        .await
        .map_err(|_| AppError::conflict("工作正在關閉"))?;
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    super::authority(&mut tx, actor, None, true).await?;
    let provider = load(&mut tx, id).await?;
    if provider.revision != revision {
        return Err(AppError::conflict("供應商已更新，請重新載入"));
    }
    let client = client(state, &provider)?;
    workflow_audit::record(
        &mut tx,
        None,
        Some(actor),
        "provider",
        id,
        "connection_test_started",
        json!({"revision":revision}),
    )
    .await?;
    tx.commit().await?;
    let result=client.complete(CompletionRequest{contract_version:1,system:"Return the synthetic connection check object only. No tools.".into(),input:json!({"check":"synthetic_connection_only"}),output_schema:json!({"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"],"additionalProperties":false}),schema_name:"connection_check".into(),max_output_tokens:128,timeout_ms:15000,request_id:format!("connection-{id}-{revision}")}).await;
    let error = match result {
        Ok(r) if r.output["ok"] == true => None,
        Ok(_) => Some("供應商未通過結構化回應測試".to_owned()),
        Err(e) => Some(e.safe_message),
    };
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    super::authority(&mut tx, actor, None, true).await?;
    let updated = sqlx::query(
        "UPDATE ai_providers SET tested_revision=?,last_test_error=? WHERE id=? AND revision=?",
    )
    .bind(error.is_none().then_some(revision))
    .bind(&error)
    .bind(id)
    .bind(revision)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::conflict("測試期間設定已更新，結果已捨棄"));
    }
    workflow_audit::record(
        &mut tx,
        None,
        Some(actor),
        "provider",
        id,
        "connection_test_finished",
        json!({"revision":revision,"ok":error.is_none()}),
    )
    .await?;
    let result = load(&mut tx, id).await?.public();
    tx.commit().await?;
    Ok(result)
}
