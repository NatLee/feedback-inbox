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
