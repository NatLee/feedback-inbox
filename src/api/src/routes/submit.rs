//! 收表單的公開網址：`/f/<key>`。不用登入，別的網站可以直接送過來。
//!
//! - `POST`：收 JSON 或一般的 HTML 表單。JSON 回 `{"ok": true}`；HTML 表單送完會轉去感謝頁。
//! - `GET`：一張可以直接用的表單（不想自己刻表單時，把連結貼出去就好）。

use std::net::SocketAddr;

use axum::{
    body::to_bytes,
    extract::{ConnectInfo, Path, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::{
    auth,
    config::origin_of,
    db,
    error::AppError,
    intake::{field_label, truncate, Submission, FIELDS, HONEYPOT},
    notify::{self, escape_html, Notice},
    routes::sites::parse_list,
    state::AppState,
};

/// 一次送出的內容上限。
const MAX_BODY_BYTES: usize = 64 * 1024;

#[derive(sqlx::FromRow)]
struct FormSite {
    id: i64,
    name: String,
    url: String,
    allowed_origins: String,
    required_fields: String,
    redirect_url: String,
    active: bool,
}

async fn find_site(state: &AppState, key: &str) -> Result<Option<FormSite>, AppError> {
    Ok(sqlx::query_as(
        "SELECT id, name, url, allowed_origins, required_fields, redirect_url, active
         FROM sites WHERE key = ?",
    )
    .bind(key)
    .fetch_optional(&state.db)
    .await?)
}

/// 這個請求能不能送進這個網站，以及回應要帶的 CORS 標頭。
struct Cors {
    allowed: bool,
    allow_origin: Option<HeaderValue>,
}

fn cors(state: &AppState, site: &FormSite, headers: &HeaderMap) -> Cors {
    let origins = parse_list(&site.allowed_origins);
    let raw = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    if origins.is_empty() {
        return Cors {
            allowed: true,
            allow_origin: Some(HeaderValue::from_static("*")),
        };
    }
    let Some(raw) = raw else {
        // 沒有 Origin 的不是瀏覽器的跨站請求（伺服器之間、命令列工具）
        return Cors {
            allowed: true,
            allow_origin: None,
        };
    };
    let allowed = match origin_of(raw) {
        // 內建的表單是從本站送出的
        Some(origin) => origins.contains(&origin) || auth::same_origin(headers, &state.config),
        None => false,
    };
    Cors {
        allowed,
        allow_origin: HeaderValue::from_str(raw).ok().filter(|_| allowed),
    }
}

fn with_cors(mut response: Response, cors: &Cors) -> Response {
    let headers = response.headers_mut();
    if let Some(origin) = &cors.allow_origin {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
    }
    headers.append(header::VARY, HeaderValue::from_static("Origin"));
    response
}

/// 訪客的 IP。前面有反向代理時（`FI_TRUST_PROXY`）看 `X-Forwarded-For` 的最後一個，
/// 也就是代理自己加上去的那個；前面幾個是對方可以亂填的。
fn client_ip(state: &AppState, req: &Request) -> String {
    if state.config.trust_proxy {
        let forwarded = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit(',').next())
            .map(str::trim)
            .filter(|ip| !ip.is_empty());
        if let Some(ip) = forwarded {
            return truncate(ip, 60);
        }
    }
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip().to_string())
        .unwrap_or_default()
}

fn header_text(headers: &HeaderMap, name: header::HeaderName, max: usize) -> String {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|v| truncate(v.trim(), max))
        .unwrap_or_default()
}

/// 送的人要的是 JSON（用 fetch 送的），還是網頁（一般的 HTML 表單）。
fn wants_json(headers: &HeaderMap, content_type: &str) -> bool {
    content_type.starts_with("application/json")
        || headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|accept| accept.contains("application/json"))
}

fn failure(json: bool, status: StatusCode, message: &str) -> Response {
    if json {
        (status, Json(json!({ "error": message }))).into_response()
    } else {
        let body = format!(
            "<h1>沒有送出去</h1><p>{}</p><p class=\"muted\">請回上一頁修改之後再送一次。</p>",
            escape_html(message)
        );
        (status, page("沒有送出去", &body)).into_response()
    }
}

pub async fn submit(
    State(state): State<AppState>,
    Path(key): Path<String>,
    req: Request,
) -> Result<Response, AppError> {
    let headers = req.headers().clone();
    let content_type = header_text(&headers, header::CONTENT_TYPE, 200).to_ascii_lowercase();
    let json = wants_json(&headers, &content_type);

    let Some(site) = find_site(&state, &key).await? else {
        return Ok(failure(json, StatusCode::NOT_FOUND, "找不到這張表單"));
    };
    let cors = cors(&state, &site, &headers);
    let fail = |status, message: &str| Ok(with_cors(failure(json, status, message), &cors));
    if !cors.allowed {
        return fail(StatusCode::FORBIDDEN, "這張表單不接受從這個網站送出");
    }
    if !site.active {
        return fail(StatusCode::FORBIDDEN, "這張表單已經關閉");
    }
    let ip = client_ip(&state, &req);
    if !state.submissions.allow(&ip) {
        return fail(StatusCode::TOO_MANY_REQUESTS, "送得太頻繁了，請稍後再試");
    }

    let Ok(body) = to_bytes(req.into_body(), MAX_BODY_BYTES).await else {
        return fail(StatusCode::PAYLOAD_TOO_LARGE, "內容太長了");
    };
    let parsed = if content_type.starts_with("application/json") {
        serde_json::from_slice::<Value>(&body)
            .map_err(|_| "內容不是合法的 JSON".to_owned())
            .and_then(|value| Submission::from_json(&value))
    } else if content_type.starts_with("application/x-www-form-urlencoded") {
        serde_urlencoded::from_bytes::<Vec<(String, String)>>(&body)
            .map(Submission::from_form)
            .map_err(|_| "表單的內容讀不懂".to_owned())
    } else {
        return fail(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "只收 application/json 或 application/x-www-form-urlencoded",
        );
    };
    let submission = match parsed {
        Ok(submission) => submission,
        Err(message) => return fail(StatusCode::BAD_REQUEST, &message),
    };

    // 機器人填了藏起來的欄位：照樣回成功，但不存
    if !submission.honeypot {
        if let Err(message) = submission.validate(&parse_list(&site.required_fields)) {
            return fail(StatusCode::BAD_REQUEST, &message);
        }
        store(&state, &site, submission, &ip, &headers).await?;
    }

    let response = if json {
        Json(json!({ "ok": true })).into_response()
    } else {
        // 轉去哪裡只看網站的設定，不看表單送來的值（不然會變成任意轉址）
        let to = if site.redirect_url.is_empty() {
            format!("/f/{key}/thanks")
        } else {
            site.redirect_url.clone()
        };
        (StatusCode::SEE_OTHER, [(header::LOCATION, to)]).into_response()
    };
    Ok(with_cors(response, &cors))
}

async fn store(
    state: &AppState,
    site: &FormSite,
    submission: Submission,
    ip: &str,
    headers: &HeaderMap,
) -> Result<(), AppError> {
    let now = db::now();
    let id = sqlx::query(
        "INSERT INTO feedback (site_id, name, email, phone, company, message, extra,
            ip, user_agent, page_url, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(site.id)
    .bind(&submission.name)
    .bind(&submission.email)
    .bind(&submission.phone)
    .bind(&submission.company)
    .bind(&submission.message)
    .bind(serde_json::to_string(&submission.extra)?)
    .bind(ip)
    .bind(header_text(headers, header::USER_AGENT, 300))
    .bind(header_text(headers, header::REFERER, 500))
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?
    .last_insert_rowid();

    let notice = Notice {
        site_name: site.name.clone(),
        feedback_id: id,
        name: submission.name,
        email: submission.email,
        phone: submission.phone,
        company: submission.company,
        message: submission.message,
        extra: submission.extra,
        created_at: now,
        link: state
            .config
            .public_url
            .as_ref()
            .map(|base| format!("{base}/inbox/{id}")),
    };
    // 通知在背景送，送表單的人不用等
    tokio::spawn(notify::deliver(state.clone(), site.id, notice));
    Ok(())
}

/// CORS 的預檢：瀏覽器用 fetch 送 JSON 之前會先問。
pub async fn preflight(
    State(state): State<AppState>,
    Path(key): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let Some(site) = find_site(&state, &key).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let cors = cors(&state, &site, &headers);
    if !cors.allowed {
        return Ok(with_cors(StatusCode::FORBIDDEN.into_response(), &cors));
    }
    let response = (
        StatusCode::NO_CONTENT,
        [
            (header::ACCESS_CONTROL_ALLOW_METHODS, "POST, OPTIONS"),
            (header::ACCESS_CONTROL_ALLOW_HEADERS, "Content-Type, Accept"),
            (header::ACCESS_CONTROL_MAX_AGE, "86400"),
        ],
    )
        .into_response();
    Ok(with_cors(response, &cors))
}

const PAGE_STYLE: &str = "
:root{color-scheme:light dark;--bg:#fafafa;--card:#fff;--line:#e4e4e7;--text:#18181b;--muted:#71717a;--brand:#0d9488;--brand-text:#fff}
@media (prefers-color-scheme:dark){:root{--bg:#09090b;--card:#18181b;--line:#3f3f46;--text:#f4f4f5;--muted:#a1a1aa;--brand:#14b8a6;--brand-text:#042f2e}}
*{box-sizing:border-box}
body{margin:0;padding:32px 16px;background:var(--bg);color:var(--text);font:15px/1.6 -apple-system,'Segoe UI','Noto Sans TC','Microsoft JhengHei',sans-serif}
main{max-width:520px;margin:0 auto;padding:28px;background:var(--card);border:1px solid var(--line);border-radius:16px}
h1{margin:0 0 4px;font-size:20px}
p{margin:8px 0}
.muted{color:var(--muted);font-size:13px}
label{display:block;margin:16px 0 6px;font-size:13px;font-weight:600}
label span{color:var(--brand)}
input,textarea{width:100%;padding:9px 12px;border:1px solid var(--line);border-radius:10px;background:transparent;color:inherit;font:inherit}
input:focus,textarea:focus{outline:2px solid var(--brand);outline-offset:1px}
textarea{min-height:140px;resize:vertical}
button,.button{display:inline-block;margin-top:20px;padding:10px 18px;border:0;border-radius:10px;background:var(--brand);color:var(--brand-text);font:inherit;font-weight:600;text-decoration:none;cursor:pointer}
.trap{position:absolute;left:-9999px}
";

/// 內建頁面的外框。`body` 要是已經跳脫過的 HTML。
fn page(title: &str, body: &str) -> Response {
    let html = format!(
        "<!doctype html><html lang=\"zh-Hant\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <meta name=\"robots\" content=\"noindex\"><title>{}</title><style>{PAGE_STYLE}</style></head>\
         <body><main>{body}</main></body></html>",
        escape_html(title)
    );
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; style-src 'unsafe-inline'",
            ),
            (header::CACHE_CONTROL, "no-store"),
        ],
        html,
    )
        .into_response()
}

fn not_found_page() -> Response {
    let mut response = page(
        "找不到這張表單",
        "<h1>找不到這張表單</h1><p class=\"muted\">網址可能打錯了，或是這張表單已經換了新的網址。</p>",
    );
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
}

/// 內建的表單。
pub async fn form(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Response, AppError> {
    let Some(site) = find_site(&state, &key).await? else {
        return Ok(not_found_page());
    };
    let name = escape_html(&site.name);
    if !site.active {
        let body =
            format!("<h1>{name}</h1><p class=\"muted\">這張表單已經關閉，不再收新的訊息。</p>");
        return Ok(page(&site.name, &body));
    }
    let required = parse_list(&site.required_fields);
    let mut fields = String::new();
    for field in FIELDS {
        let must = required.iter().any(|r| r == field);
        let attrs = format!(
            "id=\"{field}\" name=\"{field}\"{}",
            if must { " required" } else { "" }
        );
        let control = match field {
            "message" => format!("<textarea {attrs} maxlength=\"10000\"></textarea>"),
            "email" => {
                format!("<input {attrs} type=\"email\" maxlength=\"254\" autocomplete=\"email\">")
            }
            "phone" => {
                format!("<input {attrs} type=\"tel\" maxlength=\"50\" autocomplete=\"tel\">")
            }
            "name" => {
                format!("<input {attrs} type=\"text\" maxlength=\"100\" autocomplete=\"name\">")
            }
            _ => format!(
                "<input {attrs} type=\"text\" maxlength=\"200\" autocomplete=\"organization\">"
            ),
        };
        fields.push_str(&format!(
            "<label for=\"{field}\">{}{}</label>{control}",
            field_label(field),
            if must { " <span>*</span>" } else { "" }
        ));
    }
    let body = format!(
        "<h1>{name}</h1><p class=\"muted\">留下訊息，我們會盡快回覆。</p>\
         <form method=\"post\" action=\"/f/{key}\">{fields}\
         <div class=\"trap\" aria-hidden=\"true\"><label for=\"{HONEYPOT}\">這一格請留白</label>\
         <input id=\"{HONEYPOT}\" name=\"{HONEYPOT}\" type=\"text\" tabindex=\"-1\" autocomplete=\"off\"></div>\
         <button type=\"submit\">送出</button></form>",
        key = escape_html(&key),
    );
    Ok(page(&site.name, &body))
}

/// HTML 表單送出之後的感謝頁（網站沒有設定自己的轉址時用）。
pub async fn thanks(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Response, AppError> {
    let Some(site) = find_site(&state, &key).await? else {
        return Ok(not_found_page());
    };
    let back = if site.url.is_empty() {
        String::new()
    } else {
        format!(
            "<a class=\"button\" href=\"{}\">回到 {}</a>",
            escape_html(&site.url),
            escape_html(&site.name)
        )
    };
    let body =
        format!("<h1>已經收到你的訊息</h1><p class=\"muted\">謝謝，我們會盡快回覆。</p>{back}");
    Ok(page("已經收到你的訊息", &body))
}
