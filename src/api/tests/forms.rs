mod support;
use axum::http::{Method, StatusCode};
use serde_json::json;
use support::TestApp;

#[tokio::test]
async fn publish_is_immutable_and_revision_checked() {
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let site = t.site(&admin, json!({"name":"表單測試"})).await;
    let id = site["id"].as_i64().unwrap();
    let route = format!("/api/sites/{id}/form");
    let (status, state) = t.call(Some(&admin), Method::GET, &route, None).await;
    assert_eq!(status, StatusCode::OK, "{state}");
    assert_eq!(state["published"]["version"], 1);
    assert_eq!(site["origin_mode"], "exact");
    let mut schema = state["draft"]["schema"].clone();
    schema["fields"].as_array_mut().unwrap().push(json!({"id":"1234567890abcdef1234567890abcdef","key":"member_id","label":"會員編號","type":"text","required":true,"help_text":"","sensitivity":"personal","validation":{"max_length":128},"options":[]}));
    let (status, draft) = t
        .call(
            Some(&admin),
            Method::PUT,
            &format!("{route}/draft"),
            Some(json!({"expected_revision":1,"schema":schema})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    assert_eq!(draft["revision"], 2);
    let (status, _) = t
        .call(
            Some(&admin),
            Method::PUT,
            &format!("{route}/draft"),
            Some(json!({"expected_revision":1,"schema":schema})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let published = t
        .ok(
            &admin,
            Method::POST,
            &format!("{route}/publish"),
            json!({"expected_revision":2}),
        )
        .await;
    assert_eq!(published["version"], 2);
    let (status, _) = t
        .call(
            Some(&admin),
            Method::POST,
            &format!("{route}/versions/2/retire"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM form_versions WHERE site_id=?")
        .bind(id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn schema_rejects_reserved_duplicate_and_hidden_only_fields() {
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let site = t.site(&admin, json!({"name":"驗證"})).await;
    let id = site["id"].as_i64().unwrap();
    let route = format!("/api/sites/{id}/form");
    let state = t.get(&admin, &route).await;
    for bad in ["_identity_token", "fullname", "extra"] {
        let mut schema = state["draft"]["schema"].clone();
        schema["fields"][0]["key"] = json!(bad);
        let (status, _) = t
            .call(
                Some(&admin),
                Method::PUT,
                &format!("{route}/draft"),
                Some(json!({"expected_revision":1,"schema":schema})),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (member_id, member) = t.member(&admin, "formmember").await;
    t.ok(
        &admin,
        Method::PATCH,
        &format!("/api/sites/{id}"),
        json!({"member_ids":[member_id]}),
    )
    .await;
    let state = t.get(&member, &route).await;
    assert!(state.get("draft").is_none());
    let (status, _) = t
        .call(
            Some(&member),
            Method::POST,
            &format!("{route}/publish"),
            Some(json!({"expected_revision":1})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn versioned_submission_and_generated_examples_include_custom_required_fields() {
    use axum::{body::Body, http::Request};
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let site = t.site(&admin, json!({"name":"欄位一致性"})).await;
    let id = site["id"].as_i64().unwrap();
    let key = site["key"].as_str().unwrap();
    let route = format!("/api/sites/{id}/form");
    let state = t.get(&admin, &route).await;
    let mut schema = state["draft"]["schema"].clone();
    schema["fields"].as_array_mut().unwrap().push(json!({"id":"1234567890abcdef1234567890abcdef","key":"member_id","label":"會員 <編號>","type":"text","required":true,"help_text":"","sensitivity":"personal","validation":{"max_length":128},"options":[]}));
    t.ok(
        &admin,
        Method::PUT,
        &format!("{route}/draft"),
        json!({"expected_revision":1,"schema":schema}),
    )
    .await;
    t.ok(
        &admin,
        Method::POST,
        &format!("{route}/publish"),
        json!({"expected_revision":2}),
    )
    .await;
    let (status, _, html) = t
        .send(
            Request::builder()
                .uri(format!("/f/{key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let html = String::from_utf8(html).unwrap();
    assert!(html.contains("name=\"member_id\" required"));
    assert!(html.contains("會員 &lt;編號&gt;"));
    for format in ["html", "fetch", "curl", "node"] {
        let code = t
            .get(
                &admin,
                &format!("/api/sites/{id}/integrations?format={format}"),
            )
            .await;
        assert!(
            code["code"].as_str().unwrap().contains("member_id"),
            "{code}"
        );
    }
    let message =
        json!({"_form_version":2,"name":"王","email":"test@example.com","message":"hello"});
    assert_eq!(
        t.submit(key, message.clone()).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut message = message;
    message["member_id"] = json!("member-1");
    assert_eq!(t.submit(key, message).await.0, StatusCode::OK);
    t.ok(
        &admin,
        Method::POST,
        &format!("{route}/versions/1/retire"),
        json!({}),
    )
    .await;
    let (status, error) = t
        .submit(
            key,
            json!({"_form_version":1,"name":"王","email":"test@example.com","message":"old"}),
        )
        .await;
    assert_eq!(status, StatusCode::GONE);
    assert_eq!(error["code"], "form_version_retired");
}

#[test]
fn typed_answers_validate_required_number_select_checkbox() {
    use feedback_inbox::forms::*;
    let schema:FormSchema=serde_json::from_value(json!({"schema_version":1,"title":"測試","description":"","submit_label":"送出","success_message":"收到","fields":[
        {"id":"00000000000000000000000000000001","key":"count","label":"人數","type":"number","required":true,"validation":{"min":1,"max":10,"integer_only":true}},
        {"id":"00000000000000000000000000000002","key":"topic","label":"主題","type":"select","required":true,"options":[{"value":"info","label":"資訊"}]},
        {"id":"00000000000000000000000000000003","key":"reply_requested","label":"同意回覆","type":"checkbox","required":true}
    ]})).unwrap();
    validate_schema(&schema, None).unwrap();
    let valid = json!({"count":"3","topic":"info","reply_requested":"on"});
    let accepted = validate_answers(&schema, &valid, false).unwrap();
    assert_eq!(accepted.values["00000000000000000000000000000001"], 3.0);
    assert_eq!(accepted.values["00000000000000000000000000000003"], true);
    for (key, value) in [
        ("count", json!(0)),
        ("count", json!(11)),
        ("count", json!(1.5)),
        ("topic", json!("refund")),
        ("reply_requested", json!(false)),
    ] {
        let mut bad = valid.clone();
        bad[key] = value;
        assert!(validate_answers(&schema, &bad, false).is_err());
    }
}

#[tokio::test]
async fn migration_preserves_existing_feedback_and_marks_origin_mode() {
    let dir = tempfile::TempDir::new().unwrap();
    let migrations = dir.path().join("migrations");
    std::fs::create_dir(&migrations).unwrap();
    for file in [
        "0001_init.sql",
        "0002_access.sql",
        "0003_access_client_registration.sql",
        "0004_registration.sql",
    ] {
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("migrations")
                .join(file),
            migrations.join(file),
        )
        .unwrap();
    }
    let path = dir.path().join("legacy.db");
    let opts = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true);
    let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();
    sqlx::migrate::Migrator::new(migrations.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sites(id,name,key,created_at) VALUES(1,'舊表單','legacy-key','2020-01-01T00:00:00Z')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO feedback(site_id,message,created_at,updated_at) VALUES(1,'不能改動的歷史內容','2020-01-01T00:00:00Z','2020-01-01T00:00:00Z')").execute(&pool).await.unwrap();
    pool.close().await;
    let pool = feedback_inbox::db::connect(&path).await.unwrap();
    let row: (String, Option<i64>, String) =
        sqlx::query_as("SELECT message,form_version_id,submission_mode FROM feedback")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row, ("不能改動的歷史內容".into(), None, "legacy".into()));
    let mode: String = sqlx::query_scalar("SELECT origin_mode FROM sites")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(mode, "legacy_public");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM form_versions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn exact_origins_errors_iframe_and_idempotency_are_separate_controls() {
    use axum::{
        body::Body,
        http::{header, Request},
    };
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let site = t.site(&admin, json!({"name":"來源限制"})).await;
    let id = site["id"].as_i64().unwrap();
    let key = site["key"].as_str().unwrap();
    let path = format!("/f/{key}");
    let request = |origin: &str, body: &str| {
        Request::builder()
            .method("POST")
            .uri(&path)
            .header(header::ORIGIN, origin)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    };
    assert_eq!(
        t.send(request("https://client.example", "{}")).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(t.send(request("null", "{}")).await.0, StatusCode::FORBIDDEN);
    t.ok(&admin,Method::PATCH,&format!("/api/sites/{id}"),json!({"allowed_origins":["https://client.example"],"embed_origins":["https://embed.example"]})).await;
    let (status, headers, _) = t.send(request("https://client.example", "{")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        headers[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://client.example"
    );
    let (status, headers, _) = t
        .send(
            Request::builder()
                .uri(format!("{path}/embed"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!headers.contains_key(header::X_FRAME_OPTIONS));
    assert!(headers[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .contains("frame-ancestors 'self' https://embed.example"));
    let (_, headers, _) = t
        .send(Request::builder().uri(&path).body(Body::empty()).unwrap())
        .await;
    assert_eq!(headers[header::X_FRAME_OPTIONS], "DENY");
    let body = json!({"name":"訪客","email":"visitor@example.com","message":"一次就好","_submission_id":"submission-key-00001"});
    assert_eq!(t.submit(key, body.clone()).await.0, StatusCode::OK);
    assert_eq!(t.submit(key, body.clone()).await.0, StatusCode::OK);
    assert_eq!(t.feedback_count().await, 1);
    let mut changed = body;
    changed["message"] = json!("不同內容");
    assert_eq!(t.submit(key, changed).await.0, StatusCode::CONFLICT);
    assert_eq!(t.feedback_count().await, 1);
    let (_, headers, _) = t.send(request("https://evil.example", "{}")).await;
    assert!(!headers.contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
}

fn signed_identity(secret: &str, site_id: i64, jti: &str, expires_delta: i64) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let now = chrono::Utc::now().timestamp();
    let payload = json!({"v":1,"site_id":site_id,"sub":"member-42","email":"verified@example.com","email_verified":true,"iat":now,"exp":now+expires_delta,"jti":jti});
    let encoded = URL_SAFE_NO_PAD.encode(payload.to_string());
    let mut mac = Hmac::<Sha256>::new_from_slice(&URL_SAFE_NO_PAD.decode(secret).unwrap()).unwrap();
    mac.update(encoded.as_bytes());
    format!(
        "{encoded}.{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
}
#[tokio::test]
async fn identity_tokens_are_site_bound_expiring_and_single_use() {
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let site = t.site(&admin, json!({"name":"驗證身分"})).await;
    let id = site["id"].as_i64().unwrap();
    let key = site["key"].as_str().unwrap();
    let signing = t
        .ok(
            &admin,
            Method::POST,
            &format!("/api/sites/{id}/identity-key/rotate"),
            json!({}),
        )
        .await;
    let secret = signing["secret"].as_str().unwrap();
    let payload = |token: String| json!({"name":"會員","email":"verified@example.com","message":"需要資訊","_identity_token":token});
    for token in [
        signed_identity(secret, id + 1, "nonce-wrong-site-001", 300),
        signed_identity(secret, id, "nonce-expired-000001", -120),
        signed_identity(secret, id, "nonce-too-long-00001", 601),
    ] {
        assert_eq!(t.submit(key, payload(token)).await.0, StatusCode::FORBIDDEN);
    }
    let token = signed_identity(secret, id, "nonce-verified-00001", 300);
    let mut body = payload(token);
    body["_submission_id"] = json!("identity-retry-000001");
    assert_eq!(t.submit(key, body.clone()).await.0, StatusCode::OK);
    assert_eq!(t.submit(key, body.clone()).await.0, StatusCode::OK);
    body.as_object_mut().unwrap().remove("_submission_id");
    assert_eq!(t.submit(key, body).await.0, StatusCode::CONFLICT);
    let stored: String = sqlx::query_scalar("SELECT identity_json FROM feedback")
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    let identity: serde_json::Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(identity["subject"], "member-42");
    assert_eq!(identity["email_verified"], true);
    assert!(!stored.contains(secret));
}
#[tokio::test]
async fn synthetic_receipts_are_bounded_and_excluded_from_ordinary_statistics() {
    let t = TestApp::new().await;
    let admin = t.admin().await;
    let site = t.site(&admin, json!({"name":"安裝測試"})).await;
    let id = site["id"].as_i64().unwrap();
    let key = site["key"].as_str().unwrap();
    let session = t
        .ok(
            &admin,
            Method::POST,
            &format!("/api/sites/{id}/verification-sessions"),
            json!({}),
        )
        .await;
    for _ in 0..5 {
        assert_eq!(t.submit(key,json!({"name":"Test","email":"test@example.com","message":"synthetic","_test_token":session["token"]})).await.0,StatusCode::OK);
    }
    assert_eq!(t.submit(key,json!({"name":"Test","email":"test@example.com","message":"sixth","_test_token":session["token"]})).await.0,StatusCode::BAD_REQUEST);
    let normal = t.get(&admin, "/api/feedback").await;
    assert_eq!(normal["total"], 0);
    let stats = t.get(&admin, "/api/stats").await;
    assert_eq!(stats["by_status"]["new"], 0);
    let site = t.get(&admin, &format!("/api/sites/{id}")).await;
    assert_eq!(site["total"], 0);
    let deliveries: i64 = sqlx::query_scalar("SELECT count(*) FROM deliveries")
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!(deliveries, 0);
    let status = t
        .get(
            &admin,
            &format!("/api/sites/{id}/verification-sessions/{}", session["id"]),
        )
        .await;
    assert_eq!(status["received"], 5);
}
