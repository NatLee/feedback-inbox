#![allow(dead_code, unused_imports)]
// Shared integration fixture; each test owns a disposable SQLite database.

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

pub struct TestApp {
    pub router: Router,
    pub state: AppState,
    _dir: TempDir,
}

/// 某個人登入之後的 cookie。
#[derive(Clone)]
pub struct Session(String);

impl TestApp {
    pub async fn new() -> Self {
        Self::with_rate_limit(0).await
    }

    pub async fn with_rate_limit(rate_limit: u32) -> Self {
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

    pub async fn send(&self, req: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
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

    pub async fn call(
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
    pub async fn sign_in(&self, path: &str, body: Value) -> Session {
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
    pub async fn admin(&self) -> Session {
        self.sign_in(
            "/api/auth/setup",
            json!({ "username": "admin", "display_name": "管理員", "password": PASSWORD }),
        )
        .await
    }

    /// 由管理員建立一個成員並登入，回傳（編號、session）。
    pub async fn member(&self, admin: &Session, username: &str) -> (i64, Session) {
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

    pub async fn get(&self, session: &Session, path: &str) -> Value {
        let (status, value) = self.call(Some(session), Method::GET, path, None).await;
        assert_eq!(status, StatusCode::OK, "{path}: {value}");
        value
    }

    pub async fn ok(&self, session: &Session, method: Method, path: &str, body: Value) -> Value {
        let (status, value) = self.call(Some(session), method, path, Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{path}: {value}");
        value
    }

    pub async fn site(&self, admin: &Session, body: Value) -> Value {
        self.ok(admin, Method::POST, "/api/sites", body).await
    }

    /// 像別的網站用 fetch 那樣送一則訊息進來。
    pub async fn submit(&self, key: &str, body: Value) -> (StatusCode, Value) {
        self.call(None, Method::POST, &format!("/f/{key}"), Some(body))
            .await
    }

    pub async fn feedback_count(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM feedback")
            .fetch_one(&self.state.db)
            .await
            .unwrap()
    }
}
