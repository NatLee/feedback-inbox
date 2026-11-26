//! 整合測試：每個測試用一個全新的暫存資料庫，直接對 router 送請求。

use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Request, StatusCode},
    Router,
};
use feedback_inbox::{app, build_state, demo, AppState, Config};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::ServiceExt;

const PASSWORD: &str = "test-password-123";

struct TestApp {
    router: Router,
    state: AppState,
    _dir: TempDir,
}

/// 某個人登入之後的 cookie。
#[derive(Clone)]
struct Session(String);

impl TestApp {
    async fn new() -> Self {
        Self::with_rate_limit(0).await
    }

    async fn with_rate_limit(rate_limit: u32) -> Self {
        let dir = TempDir::new().unwrap();
        let config = Config {
            bind: "127.0.0.1:0".into(),
            database_path: dir.path().join("test.db"),
            static_dir: dir.path().join("web"),
            public_url: Some("https://inbox.example.com".into()),
            public_origin: Some("https://inbox.example.com".into()),
            cookie_secure: false,
            trust_proxy: false,
            rate_limit,
            smtp: None,
        };
        let state = build_state(config).await.unwrap();
        Self {
            router: app(state.clone()),
            state,
            _dir: dir,
        }
    }

    async fn send(&self, req: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
        let resp = self.router.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        (status, headers, bytes)
    }

    async fn call(
        &self,
        session: Option<&Session>,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(Session(cookie)) = session {
            req = req.header(header::COOKIE, cookie);
        }
        let req = match body {
            Some(b) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let (status, _, bytes) = self.send(req).await;
        let value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, value)
    }

    /// 送出登入或設定請求，回傳 cookie。
    async fn sign_in(&self, path: &str, body: Value) -> Session {
        let req = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, headers, bytes) = self.send(req).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        let cookie = headers[header::SET_COOKIE].to_str().unwrap();
        assert!(cookie.contains("HttpOnly"));
        Session(cookie.split(';').next().unwrap().to_owned())
    }

    /// 建立管理員 `admin` 並登入。
    async fn admin(&self) -> Session {
        self.sign_in(
            "/api/auth/setup",
            json!({ "username": "admin", "display_name": "管理員", "password": PASSWORD }),
        )
        .await
    }

    /// 由管理員建立一個成員並登入，回傳（編號、session）。
    async fn member(&self, admin: &Session, username: &str) -> (i64, Session) {
        let (status, user) = self
            .call(
                Some(admin),
                Method::POST,
                "/api/users",
                Some(json!({ "username": username, "password": PASSWORD })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{user}");
        let session = self
            .sign_in(
                "/api/auth/login",
                json!({ "username": username, "password": PASSWORD }),
            )
            .await;
        (user["id"].as_i64().unwrap(), session)
    }

    async fn get(&self, session: &Session, path: &str) -> Value {
        let (status, value) = self.call(Some(session), Method::GET, path, None).await;
        assert_eq!(status, StatusCode::OK, "{path}: {value}");
        value
    }

    async fn ok(&self, session: &Session, method: Method, path: &str, body: Value) -> Value {
        let (status, value) = self.call(Some(session), method, path, Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{path}: {value}");
        value
    }

    async fn site(&self, admin: &Session, body: Value) -> Value {
        self.ok(admin, Method::POST, "/api/sites", body).await
    }

    /// 像別的網站用 fetch 那樣送一則訊息進來。
    async fn submit(&self, key: &str, body: Value) -> (StatusCode, Value) {
        self.call(None, Method::POST, &format!("/f/{key}"), Some(body))
            .await
    }

    async fn feedback_count(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM feedback")
            .fetch_one(&self.state.db)
            .await
            .unwrap()
    }
}

fn key(site: &Value) -> &str {
    site["key"].as_str().unwrap()
}

fn ids(list: &Value) -> Vec<i64> {
    let mut ids: Vec<i64> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["id"].as_i64().unwrap())
        .collect();
    ids.sort_unstable();
    ids
}

fn message(name: &str, text: &str) -> Value {
    json!({ "name": name, "email": format!("{name}@example.com"), "message": text })
}

#[tokio::test]
async fn first_visitor_creates_the_admin_and_there_is_no_default_account() {
    let t = TestApp::new().await;
    let (status, state) = t.call(None, Method::GET, "/api/auth/state", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state, json!({ "setup_required": true, "user": null }));

    // 還沒登入：什麼都讀不到
    for path in ["/api/feedback", "/api/sites", "/api/stats", "/api/users"] {
        let (status, _) = t.call(None, Method::GET, path, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
    }
    let (status, _) = t
        .call(
            None,
            Method::POST,
            "/api/auth/login",
            Some(json!({ "username": "admin", "password": "admin" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = t
        .call(
            None,
            Method::POST,
            "/api/auth/setup",
            Some(json!({ "username": "admin", "password": "short" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let admin = t.admin().await;
    let state = t.get(&admin, "/api/auth/state").await;
    assert_eq!(state["setup_required"], false);
    assert_eq!(state["user"]["role"], "admin");
    assert_eq!(state["user"]["display_name"], "管理員");

    // 設定只能做一次
    let (status, _) = t
        .call(
            None,
            Method::POST,
            "/api/auth/setup",
            Some(json!({ "username": "other", "password": PASSWORD })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 改自己的名字和收信的信箱
    let me = t
        .ok(
            &admin,
            Method::PATCH,
            "/api/auth/me",
            json!({ "email": " admin@example.com " }),
        )
        .await;
    assert_eq!(me["email"], "admin@example.com");
    assert_eq!(me["display_name"], "管理員");
    let (status, _) = t
        .call(
            Some(&admin),
            Method::PATCH,
            "/api/auth/me",
            Some(json!({ "email": "not-an-email" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 登出之後 cookie 失效
    let (status, _) = t
        .call(Some(&admin), Method::POST, "/api/auth/logout", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = t
        .call(Some(&admin), Method::GET, "/api/feedback", None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 密碼沒有被存成明文
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users")
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert!(hash.starts_with("$argon2") && !hash.contains(PASSWORD));
}

#[tokio::test]
async fn api_writes_from_another_site_are_rejected_but_forms_are_not() {
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let request = |origin: &'static str| {
        Request::builder()
            .method(Method::POST)
            .uri("/api/sites")
            .header(header::HOST, "localhost:8787")
            .header(header::ORIGIN, origin)
            .header(header::COOKIE, &admin.0)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "name": origin }).to_string()))
            .unwrap()
    };
    let (status, _, _) = t.send(request("https://evil.example")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = t.send(request("http://localhost:8787")).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = t.send(request("https://inbox.example.com")).await;
    assert_eq!(status, StatusCode::OK);

    // 表單本來就是給別的網站送的
    let site = t.site(&admin, json!({ "name": "官網" })).await;
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/f/{}", key(&site)))
        .header(header::HOST, "localhost:8787")
        .header(header::ORIGIN, "https://www.example.com")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(message("ming", "你好").to_string()))
        .unwrap();
    let (status, headers, _) = t.send(req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
}

#[tokio::test]
async fn json_submissions_are_validated_and_stored() {
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let site = t
        .site(
            &admin,
            json!({ "name": "官網", "url": "https://www.example.com" }),
        )
        .await;
    assert_eq!(key(&site).len(), 16);
    assert_eq!(site["required_fields"], json!(["name", "email", "message"]));
    assert_eq!(site["members"][0]["username"], "admin");

    let (status, body) = t.submit("nope", message("ming", "你好")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = t
        .submit(
            key(&site),
            json!({ "name": "王小明", "message": "沒留信箱" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "請填寫Email");
    let (status, body) = t
        .submit(
            key(&site),
            json!({ "name": "王小明", "email": "ming@", "message": "x" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "Email 的格式不對");
    assert_eq!(t.feedback_count().await, 0);

    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/f/{}", key(&site)))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::USER_AGENT, "test-agent")
        .header(header::REFERER, "https://www.example.com/contact")
        .body(Body::from(
            json!({
                "name": "王小明",
                "email": "ming@example.com",
                "company_name": "範例科技",
                "text": "想了解企業方案\n謝謝",
                "extra": { "預算": "10 萬", "人數": 30 },
                "_subject": "不會存",
            })
            .to_string(),
        ))
        .unwrap();
    let (status, _, bytes) = t.send(req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap(),
        json!({ "ok": true })
    );

    let list = t.get(&admin, "/api/feedback").await;
    assert_eq!(list["total"], 1);
    let item = &list["items"][0];
    assert_eq!(item["site_name"], "官網");
    assert_eq!(item["status"], "new");
    assert_eq!(item["preview"], "想了解企業方案 謝謝");

    let detail = t
        .get(&admin, &format!("/api/feedback/{}", item["id"]))
        .await;
    assert_eq!(detail["company"], "範例科技");
    assert_eq!(detail["message"], "想了解企業方案\n謝謝");
    assert_eq!(
        detail["extra"],
        json!([{ "name": "預算", "value": "10 萬" }, { "name": "人數", "value": "30" }])
    );
    assert_eq!(detail["user_agent"], "test-agent");
    assert_eq!(detail["page_url"], "https://www.example.com/contact");
    assert_eq!(detail["notes"], json!([]));

    // 藏起來的欄位被填了：回成功，但不存
    let (status, body) = t
        .submit(
            key(&site),
            json!({ "name": "bot", "email": "bot@example.com", "message": "spam", "_gotcha": "x" }),
        )
        .await;
    assert_eq!((status, body), (StatusCode::OK, json!({ "ok": true })));
    assert_eq!(t.feedback_count().await, 1);

    // 別的格式不收；太長的也不收
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/f/{}", key(&site)))
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from("hello"))
        .unwrap();
    let (status, _, _) = t.send(req).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let (status, _) = t
        .submit(key(&site), message("ming", &"字".repeat(40_000)))
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

    // 關掉的表單不收；換了網址之後舊的失效
    t.ok(
        &admin,
        Method::PATCH,
        &format!("/api/sites/{}", site["id"]),
        json!({ "active": false }),
    )
    .await;
    let (status, body) = t.submit(key(&site), message("ming", "你好")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    t.ok(
        &admin,
        Method::PATCH,
        &format!("/api/sites/{}", site["id"]),
        json!({ "active": true }),
    )
    .await;
    let rotated = t
        .ok(
            &admin,
            Method::POST,
            &format!("/api/sites/{}/rotate-key", site["id"]),
            json!({}),
        )
        .await;
    assert_ne!(key(&rotated), key(&site));
    let (status, _) = t.submit(key(&site), message("ming", "你好")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = t.submit(key(&rotated), message("ming", "你好")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(t.feedback_count().await, 2);
}

#[tokio::test]
async fn html_forms_redirect_and_the_hosted_form_works() {
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let site = t
        .site(
            &admin,
            json!({ "name": "活動 <報名>", "required_fields": ["name", "phone"] }),
        )
        .await;
    let path = format!("/f/{}", key(&site));
    let form = |body: &'static str| {
        Request::builder()
            .method(Method::POST)
            .uri(&path)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap()
    };

    // 內建的表單：必填的欄位有標出來，網站名稱有跳脫
    let (status, headers, bytes) = t
        .send(Request::builder().uri(&path).body(Body::empty()).unwrap())
        .await;
    let html = String::from_utf8(bytes).unwrap();
    assert_eq!(status, StatusCode::OK);
    assert!(headers[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .starts_with("default-src 'none'"));
    assert!(html.contains("活動 &lt;報名&gt;") && !html.contains("<報名>"));
    assert!(html.contains("name=\"phone\" required"));
    assert!(html.contains("name=\"email\" type") && html.contains("name=\"_gotcha\""));

    let (status, headers, _) = t
        .send(form(
            "name=%E7%8E%8B%E5%B0%8F%E6%98%8E&phone=0900-000-123&interest%5B%5D=a&interest%5B%5D=b&_redirect=https%3A%2F%2Fevil.example",
        ))
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], format!("{path}/thanks").as_str());
    let (status, _, bytes) = t
        .send(
            Request::builder()
                .uri(format!("{path}/thanks"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8(bytes)
        .unwrap()
        .contains("已經收到你的訊息"));

    let list = t.get(&admin, "/api/feedback").await;
    let detail = t
        .get(&admin, &format!("/api/feedback/{}", list["items"][0]["id"]))
        .await;
    assert_eq!(detail["name"], "王小明");
    assert_eq!(
        detail["extra"],
        json!([{ "name": "interest", "value": "a, b" }])
    );

    // 少了必填的欄位：回一頁說明，不是 JSON
    let (status, headers, bytes) = t.send(form("name=x")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(headers[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("text/html"));
    assert!(String::from_utf8(bytes).unwrap().contains("請填寫電話"));

    // 設了轉址就轉去那裡；表單自己帶的網址不算
    t.ok(
        &admin,
        Method::PATCH,
        &format!("/api/sites/{}", site["id"]),
        json!({ "redirect_url": "https://www.example.com/thanks" }),
    )
    .await;
    let (status, headers, _) = t.send(form("name=x&phone=1")).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "https://www.example.com/thanks");

    let (status, _, _) = t
        .send(
            Request::builder()
                .uri("/f/unknown")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn allowed_origins_limit_who_can_post_from_a_browser() {
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let site = t
        .site(
            &admin,
            json!({ "name": "官網", "allowed_origins": ["https://www.example.com/contact"] }),
        )
        .await;
    assert_eq!(site["allowed_origins"], json!(["https://www.example.com"]));
    let path = format!("/f/{}", key(&site));
    let post = |origin: Option<&'static str>| {
        let mut req = Request::builder()
            .method(Method::POST)
            .uri(&path)
            .header(header::HOST, "inbox.internal:8787")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(origin) = origin {
            req = req.header(header::ORIGIN, origin);
        }
        req.body(Body::from(message("ming", "你好").to_string()))
            .unwrap()
    };

    let (status, headers, _) = t.send(post(Some("https://www.example.com"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://www.example.com"
    );
    assert_eq!(headers[header::VARY], "Origin");

    let (status, headers, _) = t.send(post(Some("https://evil.example"))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(!headers.contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));

    // 伺服器之間的請求沒有 Origin；內建的表單是從本站送出的
    let (status, _, _) = t.send(post(None)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = t.send(post(Some("https://inbox.example.com"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(t.feedback_count().await, 3);

    let preflight = |origin: &'static str| {
        Request::builder()
            .method(Method::OPTIONS)
            .uri(&path)
            .header(header::ORIGIN, origin)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .body(Body::empty())
            .unwrap()
    };
    let (status, headers, _) = t.send(preflight("https://www.example.com")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        headers[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://www.example.com"
    );
    assert!(headers[header::ACCESS_CONTROL_ALLOW_HEADERS]
        .to_str()
        .unwrap()
        .contains("Content-Type"));
    let (status, _, _) = t.send(preflight("https://evil.example")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn submissions_are_rate_limited() {
    let t = TestApp::with_rate_limit(3).await;
    let admin = t.admin().await;
    let site = t.site(&admin, json!({ "name": "官網" })).await;
    for _ in 0..3 {
        let (status, _) = t.submit(key(&site), message("ming", "你好")).await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, body) = t.submit(key(&site), message("ming", "你好")).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(t.feedback_count().await, 3);
}

#[tokio::test]
async fn members_only_see_the_sites_they_look_after() {
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let (amy_id, amy) = t.member(&admin, "amy").await;
    let (_, ben) = t.member(&admin, "ben").await;

    let (status, _) = t
        .call(
            Some(&amy),
            Method::POST,
            "/api/sites",
            Some(json!({ "name": "我的" })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let official = t
        .site(
            &admin,
            json!({
                "name": "官網",
                "member_ids": [amy_id],
                "webhook_url": "https://hooks.example.com/secret-token",
            }),
        )
        .await;
    let docs = t.site(&admin, json!({ "name": "文件" })).await;
    let (status, body) = t
        .call(
            Some(&admin),
            Method::POST,
            "/api/sites",
            Some(json!({ "name": "官網" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (status, _) = t
        .call(
            Some(&admin),
            Method::POST,
            "/api/sites",
            Some(json!({ "name": "x", "webhook_url": "ftp://example.com" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    t.submit(key(&official), message("ming", "官網的訊息"))
        .await;
    t.submit(key(&docs), message("lin", "文件的訊息")).await;

    // amy 只看得到官網；webhook 的網址只有管理員看得到
    let sites = t.get(&amy, "/api/sites").await;
    assert_eq!(sites.as_array().unwrap().len(), 1);
    assert_eq!(sites[0]["name"], "官網");
    assert_eq!(sites[0]["webhook_url"], "");
    assert_eq!(sites[0]["has_webhook"], true);
    assert_eq!(sites[0]["unhandled"], 1);
    let admin_view = t
        .get(&admin, &format!("/api/sites/{}", official["id"]))
        .await;
    assert_eq!(
        admin_view["webhook_url"],
        "https://hooks.example.com/secret-token"
    );
    assert_eq!(t.get(&ben, "/api/sites").await, json!([]));

    let list = t.get(&amy, "/api/feedback").await;
    assert_eq!(list["total"], 1);
    assert_eq!(list["items"][0]["site_name"], "官網");
    assert_eq!(t.get(&ben, "/api/feedback").await["total"], 0);
    let all = t.get(&admin, "/api/feedback").await;
    assert_eq!(all["total"], 2);

    // 看不到的當成不存在
    let docs_feedback = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["site_name"] == "文件")
        .unwrap()["id"]
        .clone();
    for (method, path, body) in [
        (Method::GET, format!("/api/feedback/{docs_feedback}"), None),
        (
            Method::PATCH,
            format!("/api/feedback/{docs_feedback}"),
            Some(json!({ "status": "done" })),
        ),
        (
            Method::POST,
            format!("/api/feedback/{docs_feedback}/notes"),
            Some(json!({ "body": "偷看" })),
        ),
        (Method::GET, format!("/api/sites/{}", docs["id"]), None),
    ] {
        let (status, _) = t.call(Some(&amy), method, &path, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
    let bulk = t
        .ok(
            &amy,
            Method::POST,
            "/api/feedback/bulk",
            json!({ "ids": [docs_feedback], "status": "spam" }),
        )
        .await;
    assert_eq!(bulk["updated"], 0);
    let stats = t.get(&amy, "/api/stats").await;
    assert_eq!(stats["by_status"]["new"], 1);
    assert_eq!(stats["sites"].as_array().unwrap().len(), 1);

    // 成員不能改網站、不能刪訊息、不能管帳號
    for (method, path) in [
        (Method::PATCH, format!("/api/sites/{}", official["id"])),
        (Method::DELETE, format!("/api/sites/{}", official["id"])),
        (
            Method::POST,
            format!("/api/sites/{}/rotate-key", official["id"]),
        ),
        (Method::POST, format!("/api/sites/{}/test", official["id"])),
        (
            Method::DELETE,
            format!("/api/feedback/{}", list["items"][0]["id"]),
        ),
        (Method::GET, "/api/users".to_owned()),
    ] {
        let (status, _) = t.call(Some(&amy), method, &path, Some(json!({}))).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
    }

    // 把 ben 加進文件之後他就看得到了
    let users = t.get(&admin, "/api/users").await;
    let ben_id = users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == "ben")
        .unwrap()["id"]
        .clone();
    let updated = t
        .ok(
            &admin,
            Method::PATCH,
            &format!("/api/sites/{}", docs["id"]),
            json!({ "member_ids": [ben_id] }),
        )
        .await;
    assert_eq!(updated["members"][0]["username"], "ben");
    assert_eq!(updated["members"][0]["has_email"], false);
    assert_eq!(t.get(&ben, "/api/feedback").await["total"], 1);

    // 刪掉網站，訊息也跟著不見
    let (status, _) = t
        .call(
            Some(&admin),
            Method::DELETE,
            &format!("/api/sites/{}", docs["id"]),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(t.feedback_count().await, 1);
}

#[tokio::test]
async fn statuses_notes_and_filters() {
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let (amy_id, amy) = t.member(&admin, "amy").await;
    let official = t
        .site(&admin, json!({ "name": "官網", "member_ids": [amy_id] }))
        .await;
    let docs = t
        .site(
            &admin,
            json!({ "name": "文件", "required_fields": ["message"] }),
        )
        .await;
    t.submit(key(&official), message("ming", "想了解企業方案"))
        .await;
    t.submit(key(&official), message("hua", "發票要重開")).await;
    t.submit(key(&official), message("spammer", "100%_off 優惠"))
        .await;
    t.submit(key(&docs), json!({ "message": "文件有錯字" }))
        .await;
    let all = ids(&t.get(&admin, "/api/feedback").await);
    assert_eq!(all.len(), 4);
    let (first, second, third) = (all[0], all[1], all[2]);

    // 改狀態會留一筆紀錄；改成一樣的不會
    let detail = t
        .ok(
            &amy,
            Method::PATCH,
            &format!("/api/feedback/{first}"),
            json!({ "status": "in_progress" }),
        )
        .await;
    assert_eq!(detail["status"], "in_progress");
    assert_eq!(detail["notes"][0]["kind"], "status");
    assert_eq!(detail["notes"][0]["body"], "in_progress");
    assert_eq!(detail["notes"][0]["author"], "amy");
    let detail = t
        .ok(
            &amy,
            Method::PATCH,
            &format!("/api/feedback/{first}"),
            json!({ "status": "in_progress" }),
        )
        .await;
    assert_eq!(detail["notes"].as_array().unwrap().len(), 1);
    let (status, _) = t
        .call(
            Some(&amy),
            Method::PATCH,
            &format!("/api/feedback/{first}"),
            Some(json!({ "status": "archived" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 備註
    let (status, _) = t
        .call(
            Some(&amy),
            Method::POST,
            &format!("/api/feedback/{first}/notes"),
            Some(json!({ "body": "  " })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let detail = t
        .ok(
            &amy,
            Method::POST,
            &format!("/api/feedback/{first}/notes"),
            json!({ "body": " 已經回信 " }),
        )
        .await;
    let amy_note = detail["notes"][1]["id"].clone();
    assert_eq!(detail["notes"][1]["body"], "已經回信");
    let detail = t
        .ok(
            &admin,
            Method::POST,
            &format!("/api/feedback/{first}/notes"),
            json!({ "body": "收到" }),
        )
        .await;
    let status_note = detail["notes"][0]["id"].clone();
    let admin_note = detail["notes"][2]["id"].clone();
    let list = t.get(&admin, "/api/feedback?status=in_progress").await;
    assert_eq!(list["items"][0]["note_count"], 2);

    // 只能刪自己的備註；狀態的紀錄不能刪；管理員可以刪別人的
    let (status, _) = t
        .call(
            Some(&amy),
            Method::DELETE,
            &format!("/api/notes/{admin_note}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = t
        .call(
            Some(&admin),
            Method::DELETE,
            &format!("/api/notes/{status_note}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = t
        .call(
            Some(&admin),
            Method::DELETE,
            &format!("/api/notes/{amy_note}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // 一次改很多則
    let bulk = t
        .ok(
            &admin,
            Method::POST,
            "/api/feedback/bulk",
            json!({ "ids": [second, third, 9999], "status": "done" }),
        )
        .await;
    assert_eq!(bulk["updated"], 2);
    t.ok(
        &admin,
        Method::PATCH,
        &format!("/api/feedback/{third}"),
        json!({ "status": "spam" }),
    )
    .await;

    // 篩選：預設不含垃圾訊息
    let count = |list: Value| list["total"].as_i64().unwrap();
    assert_eq!(count(t.get(&admin, "/api/feedback").await), 3);
    assert_eq!(count(t.get(&admin, "/api/feedback?status=all").await), 4);
    assert_eq!(count(t.get(&admin, "/api/feedback?status=spam").await), 1);
    assert_eq!(count(t.get(&admin, "/api/feedback?status=open").await), 2);
    assert_eq!(count(t.get(&admin, "/api/feedback?status=done").await), 1);
    let by_site = t
        .get(&admin, &format!("/api/feedback?site_id={}", docs["id"]))
        .await;
    assert_eq!(count(by_site), 1);
    assert_eq!(
        count(t.get(&admin, "/api/feedback?q=%E7%99%BC%E7%A5%A8").await),
        1
    );
    assert_eq!(count(t.get(&admin, "/api/feedback?q=hua@example").await), 1);
    // % 和 _ 是一般的字，不是萬用字元
    assert_eq!(
        count(t.get(&admin, "/api/feedback?status=all&q=100%25_off").await),
        1
    );
    assert_eq!(
        count(t.get(&admin, "/api/feedback?status=all&q=%25").await),
        1
    );
    assert_eq!(
        count(
            t.get(
                &admin,
                "/api/feedback?from=2000-01-01T00:00:00Z&to=2000-01-02T00:00:00Z"
            )
            .await
        ),
        0
    );
    let (status, _) = t
        .call(Some(&admin), Method::GET, "/api/feedback?status=nope", None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = t
        .call(
            Some(&admin),
            Method::GET,
            "/api/feedback?from=yesterday",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let paged = t.get(&admin, "/api/feedback?per_page=2&page=2").await;
    assert_eq!(paged["items"].as_array().unwrap().len(), 1);
    assert_eq!(paged["total"], 3);

    // 統計
    let stats = t.get(&admin, "/api/stats?tz_offset=480").await;
    assert_eq!(
        stats["by_status"],
        json!({ "new": 1, "in_progress": 1, "done": 1, "spam": 1 })
    );
    assert_eq!(stats["daily"].as_array().unwrap().len(), 30);
    assert_eq!(stats["daily"][29]["count"], 3);
    assert_eq!(stats["last_7_days"], 3);
    assert_eq!(stats["recent"].as_array().unwrap().len(), 3);
    let official_stats = stats["sites"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "官網")
        .unwrap();
    assert_eq!(official_stats["total"], 2);
    assert_eq!(official_stats["unhandled"], 1);

    // 管理員可以刪訊息
    let (status, _) = t
        .call(
            Some(&admin),
            Method::DELETE,
            &format!("/api/feedback/{third}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count(t.get(&admin, "/api/feedback?status=all").await), 3);
}

#[tokio::test]
async fn export_is_a_csv_that_spreadsheets_can_open_safely() {
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let site = t
        .site(
            &admin,
            json!({ "name": "官網", "required_fields": ["message"] }),
        )
        .await;
    t.submit(
        key(&site),
        json!({
            "name": "=HYPERLINK(\"https://evil.example\")",
            "message": "第一行\n第二行",
            "extra": { "預算": "10 萬" },
        }),
    )
    .await;
    t.submit(key(&site), json!({ "message": "垃圾" })).await;
    let all = ids(&t.get(&admin, "/api/feedback").await);
    t.ok(
        &admin,
        Method::PATCH,
        &format!("/api/feedback/{}", all[1]),
        json!({ "status": "spam" }),
    )
    .await;

    let req = Request::builder()
        .uri("/api/feedback/export?tz_offset=480")
        .header(header::COOKIE, &admin.0)
        .body(Body::empty())
        .unwrap();
    let (status, headers, bytes) = t.send(req).await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .contains("feedback.csv"));
    assert!(bytes.starts_with(b"\xEF\xBB\xBF"));
    let mut reader = csv::Reader::from_reader(&bytes[3..]);
    assert_eq!(
        reader.headers().unwrap().iter().collect::<Vec<_>>(),
        [
            "編號",
            "網站",
            "時間",
            "狀態",
            "姓名",
            "Email",
            "電話",
            "公司",
            "訊息",
            "其他欄位"
        ]
    );
    let rows: Vec<csv::StringRecord> = reader.records().map(Result::unwrap).collect();
    assert_eq!(rows.len(), 1, "垃圾訊息預設不匯出");
    assert_eq!(&rows[0][1], "官網");
    assert_eq!(&rows[0][3], "未處理");
    assert_eq!(&rows[0][4], "'=HYPERLINK(\"https://evil.example\")");
    assert_eq!(&rows[0][8], "第一行\n第二行");
    assert_eq!(&rows[0][9], "預算：10 萬");

    let (status, _) = t
        .call(None, Method::GET, "/api/feedback/export", None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// 一個會把收到的內容記下來的假 webhook。
async fn webhook_receiver(status: StatusCode) -> (String, Arc<Mutex<Vec<Value>>>) {
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = received.clone();
    let router = Router::new().route(
        "/hook/secret-token",
        axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
            sink.lock().unwrap().push(body);
            async move { status }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/hook/secret-token",
        listener.local_addr().unwrap()
    );
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (url, received)
}

/// 通知是在背景送的：等到結果寫進資料庫。
async fn deliveries(t: &TestApp, admin: &Session, id: i64) -> Value {
    for _ in 0..100 {
        let detail = t.get(admin, &format!("/api/feedback/{id}")).await;
        if !detail["deliveries"].as_array().unwrap().is_empty() {
            return detail["deliveries"].clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("等不到通知的結果");
}

#[tokio::test]
async fn new_messages_are_sent_to_the_webhook_and_the_result_is_recorded() {
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let (url, received) = webhook_receiver(StatusCode::OK).await;
    let site = t
        .site(&admin, json!({ "name": "官網", "webhook_url": url }))
        .await;

    t.submit(
        key(&site),
        json!({ "name": "王小明", "email": "ming@example.com", "message": "你好", "方案": "企業版" }),
    )
    .await;
    let id = ids(&t.get(&admin, "/api/feedback").await)[0];
    let log = deliveries(&t, &admin, id).await;
    assert_eq!(log.as_array().unwrap().len(), 1);
    assert_eq!(log[0]["channel"], "webhook");
    assert_eq!(log[0]["ok"], true);
    // 紀錄裡不留完整的網址
    assert!(!log[0]["target"].as_str().unwrap().contains("secret-token"));

    let payload = received.lock().unwrap()[0].clone();
    assert_eq!(payload["event"], "feedback.created");
    assert_eq!(payload["site"], "官網");
    assert_eq!(payload["feedback"]["id"], id);
    assert_eq!(payload["feedback"]["name"], "王小明");
    assert_eq!(
        payload["feedback"]["extra"],
        json!([{ "name": "方案", "value": "企業版" }])
    );
    assert_eq!(
        payload["url"],
        format!("https://inbox.example.com/inbox/{id}")
    );

    // 測試通知：馬上回報結果，不會留下訊息
    let test = t
        .ok(
            &admin,
            Method::POST,
            &format!("/api/sites/{}/test", site["id"]),
            json!({}),
        )
        .await;
    assert_eq!(test["outcomes"][0]["ok"], true);
    assert_eq!(test["smtp_configured"], false);
    assert_eq!(test["no_recipients"], true);
    assert_eq!(received.lock().unwrap().len(), 2);
    assert_eq!(t.feedback_count().await, 1);
}

#[tokio::test]
async fn a_failing_webhook_does_not_lose_the_message() {
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let (url, _) = webhook_receiver(StatusCode::SERVICE_UNAVAILABLE).await;
    let site = t
        .site(&admin, json!({ "name": "官網", "webhook_url": url }))
        .await;
    let (status, _) = t.submit(key(&site), message("ming", "你好")).await;
    assert_eq!(status, StatusCode::OK);
    let id = ids(&t.get(&admin, "/api/feedback").await)[0];
    let log = deliveries(&t, &admin, id).await;
    assert_eq!(log[0]["ok"], false);
    assert!(log[0]["detail"].as_str().unwrap().contains("503"));
}

#[tokio::test]
async fn accounts_can_be_managed_by_admins() {
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let (amy_id, amy) = t.member(&admin, "amy").await;
    t.site(&admin, json!({ "name": "官網", "member_ids": [amy_id] }))
        .await;

    let users = t.get(&admin, "/api/users").await;
    let row = users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == "amy")
        .unwrap();
    assert_eq!(row["site_count"], 1);
    assert_eq!(row["email"], "");

    let updated = t
        .ok(
            &admin,
            Method::PATCH,
            &format!("/api/users/{amy_id}"),
            json!({ "email": "amy@example.com", "display_name": "陳雅婷" }),
        )
        .await;
    assert_eq!(updated["email"], "amy@example.com");
    assert_eq!(
        t.get(&amy, "/api/auth/state").await["user"]["display_name"],
        "陳雅婷"
    );

    let (status, _) = t
        .call(
            Some(&admin),
            Method::POST,
            "/api/users",
            Some(json!({ "username": "amy", "password": PASSWORD })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 停用之後馬上被登出，也登不進來
    t.ok(
        &admin,
        Method::PATCH,
        &format!("/api/users/{amy_id}"),
        json!({ "disabled": true }),
    )
    .await;
    let (status, _) = t.call(Some(&amy), Method::GET, "/api/feedback", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = t
        .call(
            None,
            Method::POST,
            "/api/auth/login",
            Some(json!({ "username": "amy", "password": PASSWORD })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 管理員不能把自己停用
    let me = t.get(&admin, "/api/auth/state").await["user"]["id"].clone();
    let (status, _) = t
        .call(
            Some(&admin),
            Method::PATCH,
            &format!("/api/users/{me}"),
            Some(json!({ "disabled": true })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn demo_data_fills_an_empty_database_once() {
    let t = TestApp::new().await;
    let summary = demo::seed(&t.state, PASSWORD).await.unwrap();
    assert_eq!(summary.users, 3);
    assert_eq!(summary.sites, 3);
    assert!(demo::seed(&t.state, PASSWORD).await.is_err());

    let admin = t
        .sign_in(
            "/api/auth/login",
            json!({ "username": "demo", "password": PASSWORD }),
        )
        .await;
    let all = t.get(&admin, "/api/feedback?status=all&per_page=100").await;
    assert_eq!(all["total"].as_i64().unwrap(), summary.feedback as i64);
    let stats = t.get(&admin, "/api/stats").await;
    for status in ["new", "in_progress", "done", "spam"] {
        assert!(stats["by_status"][status].as_i64().unwrap() > 0, "{status}");
    }
    assert_eq!(
        t.get(&admin, "/api/sites").await.as_array().unwrap().len(),
        3
    );

    // 成員只看得到自己負責的網站
    let ben = t
        .sign_in(
            "/api/auth/login",
            json!({ "username": demo::MEMBER_USERNAMES[1], "password": PASSWORD }),
        )
        .await;
    assert_eq!(t.get(&ben, "/api/sites").await.as_array().unwrap().len(), 2);
    let visible = t.get(&ben, "/api/feedback?status=all").await["total"]
        .as_i64()
        .unwrap();
    assert!(visible > 0 && visible < summary.feedback as i64);
}

#[tokio::test]
async fn meta_and_unknown_api_paths() {
    let t = TestApp::new().await;
    let (status, meta) = t.call(None, Method::GET, "/api/meta", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(meta["smtp_configured"], false);
    assert_eq!(meta["public_url"], "https://inbox.example.com");
    let (status, body) = t.call(None, Method::GET, "/api/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "找不到這個 API");
}

#[tokio::test]
async fn concurrent_setup_creates_exactly_one_administrator() {
    let app = TestApp::new().await;
    let request = |index| {
        app.call(
            None,
            Method::POST,
            "/api/auth/setup",
            Some(json!({ "username": format!("admin{index}"), "password": PASSWORD })),
        )
    };
    let (a, b, c, d, e, f) = tokio::join!(
        request(0),
        request(1),
        request(2),
        request(3),
        request(4),
        request(5)
    );
    let results = [a, b, c, d, e, f];
    assert_eq!(
        results
            .iter()
            .filter(|(status, _)| *status == StatusCode::OK)
            .count(),
        1
    );
    assert!(results
        .iter()
        .all(|(status, _)| matches!(*status, StatusCode::OK | StatusCode::CONFLICT)));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn triage_age_counts_match_filtered_lists_and_exports() {
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let (uid, member) = t.member(&admin, "triage").await;
    let site = t
        .site(&admin, json!({"name":"Visible", "member_ids":[uid]}))
        .await;
    let hidden = t.site(&admin, json!({"name":"Hidden"})).await;
    for (i, days, status, site_id) in [
        (1, 0, "new", site["id"].as_i64().unwrap()),
        (2, 2, "in_progress", site["id"].as_i64().unwrap()),
        (3, 8, "new", site["id"].as_i64().unwrap()),
        (4, 15, "new", site["id"].as_i64().unwrap()),
        (5, 9, "done", site["id"].as_i64().unwrap()),
        (6, 9, "spam", site["id"].as_i64().unwrap()),
        (7, 9, "new", hidden["id"].as_i64().unwrap()),
    ] {
        let at = feedback_inbox::db::format_time(chrono::Utc::now() - chrono::Duration::days(days));
        sqlx::query("INSERT INTO feedback(site_id,name,message,status,created_at,updated_at) VALUES(?,?,?,?,?,?)")
            .bind(site_id).bind(format!("Letter {i}")).bind("Test").bind(status).bind(&at).bind(&at)
            .execute(&t.state.db).await.unwrap();
    }
    let stats = t.get(&member, "/api/stats").await;
    assert_eq!(
        stats["open_age"],
        json!({"fresh":1,"waiting":1,"overdue":2})
    );
    for (age, count) in [("fresh", 1), ("waiting", 1), ("overdue", 2)] {
        let list = t
            .get(
                &member,
                &format!("/api/feedback?status=open&age={age}&sort=oldest"),
            )
            .await;
        assert_eq!(list["total"], count);
        let csv = t
            .get(
                &member,
                &format!("/api/feedback/export?status=open&age={age}&sort=oldest"),
            )
            .await;
        assert_eq!(csv.as_str().unwrap().lines().count(), count as usize + 1);
        assert!(!csv.as_str().unwrap().contains("Letter 7"));
    }
    let list = t
        .get(&member, "/api/feedback?status=open&age=overdue&sort=oldest")
        .await;
    assert_eq!(list["items"][0]["name"], "Letter 4");
    for query in ["age=bogus", "sort=bogus"] {
        for path in ["/api/feedback", "/api/feedback/export"] {
            assert_eq!(
                t.call(Some(&admin), Method::GET, &format!("{path}?{query}"), None)
                    .await
                    .0,
                StatusCode::BAD_REQUEST
            );
        }
    }
}
