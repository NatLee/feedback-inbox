//! Authenticated encryption for retrievable outbound credentials. Never use access-token hashes here.
use crate::{
    config::EncryptionKey,
    error::{AppError, AppResult},
};
use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit, Nonce,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand_core::{OsRng, RngCore};

pub struct SecretStore(Aes256Gcm);
impl SecretStore {
    pub fn new(key: &EncryptionKey) -> Self {
        Self(Aes256Gcm::new((&key.0).into()))
    }
    pub fn encrypt(&self, purpose: &str, entity_id: i64, plaintext: &str) -> AppResult<String> {
        if plaintext.is_empty() || plaintext.len() > 4096 {
            return Err(AppError::bad("密鑰長度需為 1–4096 位元組"));
        }
        let mut nonce = [0u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let aad = format!("{purpose}:{entity_id}:v1");
        let encrypted = self
            .0
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| unavailable())?;
        let mut data = nonce.to_vec();
        data.extend(encrypted);
        Ok(format!("v1.{}", URL_SAFE_NO_PAD.encode(data)))
    }
    pub fn decrypt(&self, purpose: &str, entity_id: i64, ciphertext: &str) -> AppResult<String> {
        if ciphertext.len() > 8192 {
            return Err(unavailable());
        }
        let data = ciphertext
            .strip_prefix("v1.")
            .and_then(|s| URL_SAFE_NO_PAD.decode(s).ok())
            .filter(|v| v.len() >= 28)
            .ok_or_else(unavailable)?;
        let aad = format!("{purpose}:{entity_id}:v1");
        let value = self
            .0
            .decrypt(
                Nonce::from_slice(&data[..12]),
                Payload {
                    msg: &data[12..],
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| unavailable())?;
        String::from_utf8(value).map_err(|_| unavailable())
    }
}
pub fn for_state(state: &crate::state::AppState) -> AppResult<SecretStore> {
    state
        .config
        .workflow
        .secret_key
        .as_ref()
        .map(SecretStore::new)
        .ok_or_else(|| {
            AppError::coded(
                503,
                "secret_key_unavailable",
                "請先設定有效的 FI_AI_SECRET_KEY，才能安全保存密鑰",
            )
        })
}
fn unavailable() -> AppError {
    AppError::coded(
        503,
        "secret_unavailable",
        "密鑰無法解密，請檢查部署加密金鑰",
    )
}
/// Offline maintenance: replace only encryption envelopes in one transaction, never print secrets.
pub async fn reencrypt(
    state: &crate::state::AppState,
    new_key: &EncryptionKey,
) -> AppResult<usize> {
    let old = for_state(state)?;
    let new = SecretStore::new(new_key);
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_jobs WHERE state='running' AND lease_until>=?)",
    )
    .bind(chrono::Utc::now().timestamp())
    .fetch_one(&mut *tx)
    .await?;
    if active {
        return Err(AppError::conflict(
            "仍有工作正在執行；請先停止服務並等待工作租約結束",
        ));
    }
    let providers: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id,credential_ciphertext FROM ai_providers WHERE credential_ciphertext IS NOT NULL",
    )
    .fetch_all(&mut *tx)
    .await?;
    let identities: Vec<(i64, String)> =
        sqlx::query_as("SELECT site_id,ciphertext FROM identity_keys")
            .fetch_all(&mut *tx)
            .await?;
    let count = providers.len() + identities.len();
    for (id, ciphertext) in providers {
        let plaintext = old.decrypt("provider", id, &ciphertext)?;
        let updated = new.encrypt("provider", id, &plaintext)?;
        if new.decrypt("provider", id, &updated)? != plaintext {
            return Err(unavailable());
        }
        sqlx::query("UPDATE ai_providers SET credential_ciphertext=?,revision=revision+1,tested_revision=NULL WHERE id=?").bind(updated).bind(id).execute(&mut *tx).await?;
    }
    for (id, ciphertext) in identities {
        let plaintext = old.decrypt("identity", id, &ciphertext)?;
        let updated = new.encrypt("identity", id, &plaintext)?;
        sqlx::query("UPDATE identity_keys SET ciphertext=? WHERE site_id=?")
            .bind(updated)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE workflow_jobs SET state='cancelled',token=NULL,lease_until=NULL WHERE state='queued' AND kind IN ('analysis','reply')").execute(&mut *tx).await?;
    sqlx::query("UPDATE ai_runs SET state='superseded',error_code='encryption_key_rotated' WHERE state IN ('queued','running')").execute(&mut *tx).await?;
    sqlx::query("UPDATE outbound_replies SET state='cancelled',error_code='encryption_key_rotated' WHERE state='queued'").execute(&mut *tx).await?;
    crate::workflow_audit::record(
        &mut tx,
        None,
        None,
        "encryption_key",
        0,
        "rotated",
        serde_json::json!({"credential_count":count}),
    )
    .await?;
    tx.commit().await?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encryption_is_random_and_bound_to_purpose_entity_and_key() {
        let store = SecretStore::new(&EncryptionKey([7; 32]));
        let a = store.encrypt("identity", 1, "private-value").unwrap();
        let b = store.encrypt("identity", 1, "private-value").unwrap();
        assert_ne!(a, b);
        assert!(!a.contains("private-value"));
        assert_eq!(store.decrypt("identity", 1, &a).unwrap(), "private-value");
        assert!(store.decrypt("provider", 1, &a).is_err());
        assert!(store.decrypt("identity", 2, &a).is_err());
        assert!(SecretStore::new(&EncryptionKey([8; 32]))
            .decrypt("identity", 1, &a)
            .is_err());
        assert!(!format!("{:?}", EncryptionKey([7; 32])).contains('7'));
    }
}
