use crate::{
    ai::secrets,
    auth, db,
    error::{AppError, AppResult},
    state::AppState,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claims {
    v: u16,
    site_id: i64,
    sub: String,
    email: Option<String>,
    #[serde(default)]
    email_verified: bool,
    iat: i64,
    exp: i64,
    jti: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct VerifiedIdentity {
    pub subject: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub verified_at: i64,
    #[serde(skip)]
    pub expires_at: i64,
    #[serde(skip)]
    pub jti: String,
    #[serde(skip)]
    pub key_fingerprint: String,
}
fn denied() -> AppError {
    AppError::forbidden("會員身分憑證無效或已過期")
}
pub fn verify_identity(
    secret: &str,
    token: &str,
    site_id: i64,
    now: i64,
) -> AppResult<VerifiedIdentity> {
    if token.len() > 4096 {
        return Err(denied());
    }
    let (encoded, signature) = token.split_once('.').ok_or_else(denied)?;
    let key = URL_SAFE_NO_PAD.decode(secret).map_err(|_| denied())?;
    let signature = URL_SAFE_NO_PAD.decode(signature).map_err(|_| denied())?;
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).map_err(|_| denied())?;
    mac.update(encoded.as_bytes());
    mac.verify_slice(&signature).map_err(|_| denied())?;
    let claims: Claims =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).map_err(|_| denied())?)
            .map_err(|_| denied())?;
    if claims.v != 1
        || claims.site_id != site_id
        || claims.sub.is_empty()
        || claims.sub.chars().count() > 128
        || claims.sub.chars().any(char::is_control)
        || claims.exp <= claims.iat
        || claims.exp.saturating_sub(claims.iat) > 600
        || claims.iat > now + 60
        || claims.exp < now - 60
        || !(16..=80).contains(&claims.jti.len())
        || !claims
            .jti
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
        || claims
            .email
            .as_ref()
            .is_some_and(|e| !crate::intake::looks_like_email(e) || e.len() > 254)
        || (claims.email_verified && claims.email.is_none())
    {
        return Err(denied());
    }
    Ok(VerifiedIdentity {
        subject: claims.sub,
        email: claims.email,
        email_verified: claims.email_verified,
        verified_at: now,
        expires_at: claims.exp + 60,
        jti: claims.jti,
        key_fingerprint: String::new(),
    })
}
pub async fn verify_for_site(
    state: &AppState,
    site_id: i64,
    token: &str,
) -> AppResult<VerifiedIdentity> {
    let stored: Option<String> =
        sqlx::query_scalar("SELECT ciphertext FROM identity_keys WHERE site_id=?")
            .bind(site_id)
            .fetch_optional(&state.db)
            .await?;
    let ciphertext = stored.ok_or_else(denied)?;
    let secret = secrets::for_state(state)?.decrypt("identity", site_id, &ciphertext)?;
    let mut identity = verify_identity(&secret, token, site_id, chrono::Utc::now().timestamp())?;
    identity.key_fingerprint = auth::sha256_hex(&ciphertext);
    Ok(identity)
}
pub async fn rotate(state: &AppState, site_id: i64, actor: i64) -> AppResult<Value> {
    let store = secrets::for_state(state)?;
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    let secret = URL_SAFE_NO_PAD.encode(key);
    let ciphertext = store.encrypt("identity", site_id, &secret)?;
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query("INSERT INTO identity_keys(site_id,ciphertext,created_at) VALUES(?,?,?) ON CONFLICT(site_id) DO UPDATE SET ciphertext=excluded.ciphertext,created_at=excluded.created_at").bind(site_id).bind(ciphertext).bind(db::now()).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO workflow_audit(site_id,actor_id,entity_type,entity_id,action,created_at) VALUES(?,?,'identity_key',?,'rotated',?)").bind(site_id).bind(actor).bind(site_id.to_string()).bind(db::now()).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(
        json!({"secret":secret,"algorithm":"HMAC-SHA256","ttl_seconds":300,"example":node_example(site_id)}),
    )
}
pub fn node_example(site_id: i64) -> String {
    format!(
        r#"// Run only in your authenticated server route; never ship this secret to the browser.
import {{createHmac,randomUUID}} from 'node:crypto';
const now = Math.floor(Date.now()/1000);
const claims = {{v:1,site_id:{site_id},sub:authenticatedUser.id,
  email:authenticatedUser.email,email_verified:authenticatedUser.emailVerified,
  iat:now,exp:now+300,jti:randomUUID()}};
const payload = Buffer.from(JSON.stringify(claims)).toString('base64url');
const signature = createHmac('sha256',Buffer.from(process.env.FEEDBACK_IDENTITY_SECRET,'base64url'))
  .update(payload).digest('base64url');
const identityToken = payload+'.'+signature;
// Include _identity_token in the POST body. It is valid for one submission.
"#
    )
}
pub async fn consume(
    tx: &mut sqlx::SqliteConnection,
    site_id: i64,
    identity: &VerifiedIdentity,
) -> AppResult<()> {
    let current: Option<String> =
        sqlx::query_scalar("SELECT ciphertext FROM identity_keys WHERE site_id=?")
            .bind(site_id)
            .fetch_optional(&mut *tx)
            .await?;
    if identity.expires_at < chrono::Utc::now().timestamp()
        || current.is_none_or(|v| auth::sha256_hex(&v) != identity.key_fingerprint)
    {
        return Err(denied());
    }
    let inserted =
        sqlx::query("INSERT OR IGNORE INTO identity_nonces(site_id,jti,expires_at) VALUES(?,?,?)")
            .bind(site_id)
            .bind(&identity.jti)
            .bind(identity.expires_at)
            .execute(tx)
            .await?;
    if inserted.rows_affected() != 1 {
        return Err(AppError::coded(
            409,
            "identity_token_replayed",
            "此會員憑證已使用，請重新取得",
        ));
    }
    Ok(())
}
pub async fn create_verification(state: &AppState, site_id: i64, actor: i64) -> AppResult<Value> {
    let token = auth::random_hex(24);
    let expires = chrono::Utc::now().timestamp() + 600;
    let id=sqlx::query("INSERT INTO verification_sessions(site_id,token_hash,expires_at,created_by) VALUES(?,?,?,?)").bind(site_id).bind(auth::sha256_hex(&token)).bind(expires).bind(actor).execute(&state.db).await?.last_insert_rowid();
    Ok(json!({"id":id,"token":token,"expires_at":expires,"max_receipts":5}))
}
pub async fn consume_verification(
    tx: &mut sqlx::SqliteConnection,
    site_id: i64,
    token: &str,
) -> AppResult<()> {
    if token.len() > 128 {
        return Err(AppError::bad("測試憑證無效"));
    }
    let updated=sqlx::query("UPDATE verification_sessions SET received=received+1,last_received_at=? WHERE site_id=? AND token_hash=? AND expires_at>? AND received<5").bind(db::now()).bind(site_id).bind(auth::sha256_hex(token)).bind(chrono::Utc::now().timestamp()).execute(tx).await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::bad("測試憑證過期或已收滿 5 則測試"));
    }
    Ok(())
}
