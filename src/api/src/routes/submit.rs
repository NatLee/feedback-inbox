//! Public versioned intake. Browser origins, iframe framing and identity are separate controls.
use crate::{
    auth,
    config::origin_of,
    db,
    error::{AppError, AppResult},
    forms::{self, FormVersion, NormalizedAnswers},
    intake::{truncate, HONEYPOT},
    notify::{self, escape_html},
    routes::sites::parse_list,
    state::AppState,
};
use axum::{
    body::to_bytes,
    extract::{ConnectInfo, Path, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};
use std::net::SocketAddr;
const MAX_BODY_BYTES: usize = 65_536;
#[derive(sqlx::FromRow)]
struct FormSite {
    id: i64,
    name: String,
    url: String,
    allowed_origins: String,
    origin_mode: String,
    embed_origins: String,
    redirect_url: String,
    active: bool,
}
async fn find_site(state: &AppState, key: &str) -> AppResult<Option<FormSite>> {
    Ok(sqlx::query_as("SELECT id,name,url,allowed_origins,origin_mode,embed_origins,redirect_url,active FROM sites WHERE key=?").bind(key).fetch_optional(&state.db).await?)
}
struct Cors {
    allowed: bool,
    allow_origin: Option<HeaderValue>,
}
fn cors(state: &AppState, site: &FormSite, headers: &HeaderMap) -> Cors {
    if site.origin_mode == "legacy_public" {
        return Cors {
            allowed: true,
            allow_origin: Some(HeaderValue::from_static("*")),
        };
    }
    let Some(raw) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return Cors {
            allowed: true,
            allow_origin: None,
        };
    };
    let allowed = origin_of(raw).is_some_and(|o| {
        parse_list(&site.allowed_origins).contains(&o) || auth::same_origin(headers, &state.config)
    });
    Cors {
        allowed,
        allow_origin: HeaderValue::from_str(raw).ok().filter(|_| allowed),
    }
}
fn with_cors(mut response: Response, cors: &Cors) -> Response {
    if let Some(origin) = &cors.allow_origin {
        response
            .headers_mut()
            .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
    }
    response
        .headers_mut()
        .append(header::VARY, HeaderValue::from_static("Origin"));
    response
}
/// Only this private response extension exempts an explicitly rendered public embed from DENY.
#[derive(Clone)]
pub(super) struct Frameable;
fn page(title: &str, body: &str, embed: Option<&[String]>) -> Response {
    let mut policy =
        "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; base-uri 'none'"
            .to_owned();
    if let Some(origins) = embed {
        policy.push_str("; frame-ancestors 'self'");
        for origin in origins {
            policy.push(' ');
            policy.push_str(origin);
        }
    }
    let mut response = (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        forms::render::document(title, body),
    )
        .into_response();
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&policy).expect("normalized origins"),
    );
    if embed.is_some() {
        response.extensions_mut().insert(Frameable);
    }
    response
}
fn failure(
    error: AppError,
    json: bool,
    embed: Option<&[String]>,
    retry: Option<(&FormVersion, &str, &str, &Value)>,
) -> Response {
    let (status, message, code) = error.public_parts();
    if json {
        let mut value = json!({"error":message});
        if let Some(code) = code {
            value["code"] = json!(code);
        }
        (status, Json(value)).into_response()
    } else {
        let body = if let Some((version, action, id, values)) = retry {
            forms::render::form(version, action, id, values, Some(&message))
        } else {
            format!("<h1>沒有送出去</h1><p class=\"error\">{}</p><p class=\"muted\">請重新開啟表單網址，或稍後再試。</p>",escape_html(&message))
        };
        let mut response = page("沒有送出去", &body, embed);
        *response.status_mut() = status;
        response
    }
}
fn header_text(headers: &HeaderMap, name: header::HeaderName, max: usize) -> String {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|v| truncate(v.trim(), max))
        .unwrap_or_default()
}
fn client_ip(state: &AppState, req: &Request) -> String {
    if state.config.trust_proxy {
        if let Some(ip) = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit(',').next())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return truncate(ip, 60);
        }
    }
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|i| i.0.ip().to_string())
        .unwrap_or_default()
}
fn parse_form(body: &[u8]) -> AppResult<Value> {
    let pairs: Vec<(String, String)> =
        serde_urlencoded::from_bytes(body).map_err(|_| AppError::bad("表單內容讀不懂"))?;
    let mut out = Map::new();
    for (key, value) in pairs {
        let key = key.trim_end_matches("[]").to_owned();
        if let Some(previous) = out.remove(&key) {
            let mut list = match previous {
                Value::Array(a) => a,
                v => vec![v],
            };
            list.push(json!(value));
            out.insert(key, Value::Array(list));
        } else {
            out.insert(key, json!(value));
        }
    }
    Ok(Value::Object(out))
}
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(m) => {
            let sorted: std::collections::BTreeMap<_, _> =
                m.iter().map(|(k, v)| (k, canonical(v))).collect();
            serde_json::to_value(sorted).expect("JSON values")
        }
        Value::Array(a) => Value::Array(a.iter().map(canonical).collect()),
        v => v.clone(),
    }
}
fn submission_key(input: &Value, headers: &HeaderMap) -> AppResult<Option<String>> {
    let body = input.get("_submission_id").and_then(Value::as_str);
    let header = headers.get("idempotency-key").and_then(|v| v.to_str().ok());
    if body.is_some() && header.is_some() && body != header {
        return Err(AppError::bad("送出識別碼不一致"));
    }
    let Some(key) = body.or(header) else {
        return Ok(None);
    };
    if !(16..=80).contains(&key.len())
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
    {
        return Err(AppError::bad("送出識別碼格式不符"));
    }
    Ok(Some(key.into()))
}
async fn receipt(state: &AppState, site_id: i64, key: Option<&str>, hash: &str) -> AppResult<bool> {
    let Some(key) = key else { return Ok(false) };
    let old:Option<String>=sqlx::query_scalar("SELECT request_hash FROM submission_receipts WHERE site_id=? AND submission_key=? AND expires_at>?").bind(site_id).bind(key).bind(chrono::Utc::now().timestamp()).fetch_optional(&state.db).await?;
    match old {
        Some(old) if old != hash => Err(AppError::conflict("相同送出識別碼不能用於不同內容")),
        Some(_) => Ok(true),
        None => Ok(false),
    }
}
pub async fn submit(
    State(state): State<AppState>,
    Path(key): Path<String>,
    req: Request,
) -> Response {
    let headers = req.headers().clone();
    let content_type = header_text(&headers, header::CONTENT_TYPE, 200).to_ascii_lowercase();
    let wants_json = content_type.starts_with("application/json")
        || headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|s| s.contains("application/json"));
    let embed_mode = req.uri().path().ends_with("/embed");
    let ip = client_ip(&state, &req);
    let site = match find_site(&state, &key).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return with_cors(
                failure(
                    AppError::not_found("找不到這張表單"),
                    wants_json,
                    None,
                    None,
                ),
                &Cors {
                    allowed: true,
                    allow_origin: Some(HeaderValue::from_static("*")),
                },
            )
        }
        Err(e) => return failure(e, wants_json, None, None),
    };
    let cors = cors(&state, &site, &headers);
    let origins = parse_list(&site.embed_origins);
    let embed = embed_mode.then_some(origins.as_slice());
    let fail = |e| with_cors(failure(e, wants_json, embed, None), &cors);
    if !cors.allowed {
        return fail(AppError::forbidden("這張表單不接受從這個網站送出"));
    }
    if !site.active {
        return fail(AppError::forbidden("這張表單已經關閉"));
    }
    if embed_mode && origins.is_empty() {
        return fail(AppError::forbidden("這張表單尚未開放嵌入"));
    }
    let body = match to_bytes(req.into_body(), MAX_BODY_BYTES).await {
        Ok(b) => b,
        Err(_) => return fail(AppError::coded(413, "body_too_large", "內容太長了")),
    };
    let input = if content_type.starts_with("application/json") {
        serde_json::from_slice::<Value>(&body).map_err(|_| AppError::bad("內容不是合法的 JSON"))
    } else if content_type.starts_with("application/x-www-form-urlencoded") {
        parse_form(&body)
    } else {
        Err(AppError::coded(
            415,
            "unsupported_media_type",
            "只收 application/json 或 application/x-www-form-urlencoded",
        ))
    };
    let mut input = match input {
        Ok(v) if v.is_object() => v,
        Ok(_) => return fail(AppError::bad("內容要是 JSON 物件")),
        Err(e) => return fail(e),
    };
    let submission_id = match submission_key(&input, &headers) {
        Ok(id) => id,
        Err(e) => return fail(e),
    };
    let hash = auth::sha256_hex(&canonical(&input).to_string());
    let repeated = match receipt(&state, site.id, submission_id.as_deref(), &hash).await {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    if !repeated {
        if !state.submissions.allow(&ip) {
            return fail(AppError::TooManyRequests("送得太頻繁了，請稍後再試".into()));
        }
        let trap = input
            .get(HONEYPOT)
            .is_some_and(|v| !v.is_null() && v.as_str().is_none_or(|s| !s.trim().is_empty()));
        if !trap {
            let requested = match input.get("_form_version") {
                None => None,
                Some(v) => match v
                    .as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                {
                    Some(n) if n > 0 => Some(n),
                    _ => return fail(AppError::bad("表單版本格式不符")),
                },
            };
            let version = match forms::load_version(&state.db, site.id, requested).await {
                Ok(v) => v,
                Err(e) => return fail(e),
            };
            let identity = match input.get("_identity_token") {
                None => None,
                Some(value) => {
                    let Some(token) = value.as_str() else {
                        return fail(AppError::bad("會員憑證格式不符"));
                    };
                    match forms::identity::verify_for_site(&state, site.id, token).await {
                        Ok(identity) => Some(identity),
                        Err(e) => return fail(e),
                    }
                }
            };
            if let Some(email) = identity
                .as_ref()
                .and_then(|identity| identity.email.as_ref())
            {
                let submitted = match crate::intake::Submission::from_json(&input) {
                    Ok(value) => value,
                    Err(e) => return fail(AppError::bad(e)),
                };
                if !submitted.email.is_empty() && !submitted.email.eq_ignore_ascii_case(email) {
                    return fail(AppError::bad("Email 與已驗證會員身分不符"));
                }
                let object = input.as_object_mut().expect("object checked");
                object.insert("email".into(), json!(email));
                object.remove("mail");
                object.remove("e-mail");
            }
            let test_token = match input.get("_test_token") {
                None => None,
                Some(value) => match value.as_str() {
                    Some(value) => Some(value.to_owned()),
                    None => return fail(AppError::bad("測試憑證格式不符")),
                },
            };
            let answers =
                match forms::validate_answers(&version.schema, &input, requested.is_none()) {
                    Ok(a) => a,
                    Err(e) => {
                        let action = format!("/f/{key}{}", if embed_mode { "/embed" } else { "" });
                        let id = submission_id
                            .clone()
                            .unwrap_or_else(|| auth::random_hex(16));
                        return with_cors(
                            failure(e, wants_json, embed, Some((&version, &action, &id, &input))),
                            &cors,
                        );
                    }
                };
            if let Err(e) = store(
                &state,
                &site,
                &version,
                answers,
                &ip,
                &headers,
                submission_id.as_deref(),
                &hash,
                requested.is_none(),
                identity.as_ref(),
                test_token.as_deref(),
            )
            .await
            {
                return fail(e);
            }
        }
    }
    let response = if wants_json {
        Json(json!({"ok":true})).into_response()
    } else {
        let to = if embed_mode {
            format!("/f/{key}/embed/thanks")
        } else if site.redirect_url.is_empty() {
            format!("/f/{key}/thanks")
        } else {
            site.redirect_url.clone()
        };
        (StatusCode::SEE_OTHER, [(header::LOCATION, to)]).into_response()
    };
    with_cors(response, &cors)
}
#[allow(clippy::too_many_arguments)]
async fn store(
    state: &AppState,
    site: &FormSite,
    version: &FormVersion,
    answers: NormalizedAnswers,
    ip: &str,
    headers: &HeaderMap,
    submission_key: Option<&str>,
    hash: &str,
    legacy: bool,
    identity: Option<&forms::identity::VerifiedIdentity>,
    test_token: Option<&str>,
) -> AppResult<()> {
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    if let Some(key) = submission_key {
        sqlx::query("DELETE FROM submission_receipts WHERE site_id=? AND submission_key=? AND expires_at<=?").bind(site.id).bind(key).bind(chrono::Utc::now().timestamp()).execute(&mut *tx).await?;
        let old: Option<String> = sqlx::query_scalar(
            "SELECT request_hash FROM submission_receipts WHERE site_id=? AND submission_key=?",
        )
        .bind(site.id)
        .bind(key)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(old) = old {
            if old != hash {
                return Err(AppError::conflict("相同送出識別碼不能用於不同內容"));
            }
            return Ok(());
        }
    }
    let (active, current): (bool, Option<i64>) =
        sqlx::query_as("SELECT active,published_form_version_id FROM sites WHERE id=?")
            .bind(site.id)
            .fetch_one(&mut *tx)
            .await?;
    if !active {
        return Err(AppError::forbidden("這張表單已經關閉"));
    }
    if legacy && current != Some(version.id) {
        return Err(AppError::conflict("表單已更新，請重新載入後送出"));
    }
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM form_versions WHERE id=? AND site_id=? AND retired_at IS NULL)").bind(version.id).bind(site.id).fetch_one(&mut *tx).await?;
    if !valid {
        return Err(AppError::coded(
            410,
            "form_version_retired",
            "這個表單版本已停用，請重新開啟表單網址",
        ));
    }
    if let Some(identity) = identity {
        forms::identity::consume(&mut tx, site.id, identity).await?;
    }
    if let Some(token) = test_token {
        forms::identity::consume_verification(&mut tx, site.id, token).await?;
    }
    let now = db::now();
    let s = answers.submission;
    let id=sqlx::query("INSERT INTO feedback(site_id,name,email,phone,company,message,extra,ip,user_agent,page_url,created_at,updated_at,form_version_id,answers_json,submission_mode,identity_json,is_test) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
 .bind(site.id).bind(&s.name).bind(&s.email).bind(&s.phone).bind(&s.company).bind(&s.message).bind(serde_json::to_string(&s.extra)?).bind(ip).bind(header_text(headers,header::USER_AGENT,300)).bind(header_text(headers,header::REFERER,500)).bind(&now).bind(&now).bind(version.id).bind(answers.values.to_string()).bind(if legacy{"legacy_unversioned"}else{"versioned"}).bind(identity.map(serde_json::to_value).transpose()?.unwrap_or(json!({})).to_string()).bind(test_token.is_some()).execute(&mut *tx).await?.last_insert_rowid();
    if let Some(key) = submission_key {
        sqlx::query("INSERT INTO submission_receipts(site_id,submission_key,request_hash,feedback_id,expires_at) VALUES(?,?,?,?,?)").bind(site.id).bind(key).bind(hash).bind(id).bind(chrono::Utc::now().timestamp()+7*86400).execute(&mut *tx).await?;
    }
    if test_token.is_none() {
        notify::enqueue(&mut tx, site.id, id).await?;
        let settings = crate::ai::policy::load(&mut tx, site.id).await?;
        if settings.enabled && settings.generate_on_receipt {
            if let Some(actor) = settings.updated_by {
                sqlx::query("SAVEPOINT ai_intake").execute(&mut *tx).await?;
                if crate::ai::analysis::enqueue(&mut tx, state, id, actor)
                    .await
                    .is_err()
                {
                    sqlx::query("ROLLBACK TO ai_intake")
                        .execute(&mut *tx)
                        .await?;
                    crate::workflow_audit::record(
                        &mut tx,
                        Some(site.id),
                        Some(actor),
                        "feedback",
                        id,
                        "automatic_analysis_skipped",
                        json!({"reason":"configuration_or_input_unavailable"}),
                    )
                    .await?;
                }
                sqlx::query("RELEASE ai_intake").execute(&mut *tx).await?;
            }
        }
    }
    tx.commit().await?;
    Ok(())
}
pub async fn preflight(
    State(state): State<AppState>,
    Path(key): Path<String>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let Some(site) = find_site(&state, &key).await? else {
        return Ok(with_cors(
            StatusCode::NOT_FOUND.into_response(),
            &Cors {
                allowed: true,
                allow_origin: Some(HeaderValue::from_static("*")),
            },
        ));
    };
    let cors = cors(&state, &site, &headers);
    if !cors.allowed {
        return Ok(with_cors(StatusCode::FORBIDDEN.into_response(), &cors));
    }
    Ok(with_cors(
        (
            StatusCode::NO_CONTENT,
            [
                (header::ACCESS_CONTROL_ALLOW_METHODS, "POST, OPTIONS"),
                (
                    header::ACCESS_CONTROL_ALLOW_HEADERS,
                    "Content-Type, Accept, Idempotency-Key",
                ),
                (header::ACCESS_CONTROL_MAX_AGE, "600"),
            ],
        )
            .into_response(),
        &cors,
    ))
}
pub async fn form(
    State(state): State<AppState>,
    Path(key): Path<String>,
    req: Request,
) -> AppResult<Response> {
    let embed_mode = req.uri().path().ends_with("/embed");
    let Some(site) = find_site(&state, &key).await? else {
        return Ok(failure(
            AppError::not_found("找不到這張表單"),
            false,
            None,
            None,
        ));
    };
    let origins = parse_list(&site.embed_origins);
    if embed_mode && origins.is_empty() {
        return Ok(failure(
            AppError::forbidden("這張表單尚未開放嵌入"),
            false,
            None,
            None,
        ));
    }
    let embed = embed_mode.then_some(origins.as_slice());
    if !site.active {
        return Ok(page(
            &site.name,
            "<h1>這張表單已經關閉</h1><p>不再收新的訊息。</p>",
            embed,
        ));
    }
    let version = forms::load_version(&state.db, site.id, None).await?;
    let action = format!("/f/{key}{}", if embed_mode { "/embed" } else { "" });
    Ok(page(
        &site.name,
        &forms::render::form(&version, &action, &auth::random_hex(16), &Value::Null, None),
        embed,
    ))
}
pub async fn thanks(
    State(state): State<AppState>,
    Path(key): Path<String>,
    req: Request,
) -> AppResult<Response> {
    let Some(site) = find_site(&state, &key).await? else {
        return Ok(failure(
            AppError::not_found("找不到這張表單"),
            false,
            None,
            None,
        ));
    };
    let version = forms::load_version(&state.db, site.id, None).await?;
    let origins = parse_list(&site.embed_origins);
    let embed = req
        .uri()
        .path()
        .ends_with("/embed/thanks")
        .then_some(origins.as_slice());
    let back = if site.url.is_empty() {
        String::new()
    } else {
        format!(
            "<a class=\"button\" href=\"{}\" target=\"_blank\" rel=\"noopener\">回到 {}</a>",
            escape_html(&site.url),
            escape_html(&site.name)
        )
    };
    Ok(page("已經收到你的訊息",&format!("<span class=\"stamp\">RECEIVED / 收件回執</span><h1>已經收到你的訊息</h1><p>{}</p>{back}",escape_html(&version.schema.success_message)),embed))
}
