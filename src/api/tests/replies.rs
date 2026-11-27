mod support;
use axum::http::{Method, StatusCode};
use serde_json::json;
use support::TestApp;
#[tokio::test]
async fn draft_edits_are_revisioned_and_smtp_is_required_to_approve() {
    let app = TestApp::new().await;
    let admin = app.admin().await;
    let site = app.site(&admin, json!({"name":"Replies"})).await;
    app.submit(
        site["key"].as_str().unwrap(),
        json!({"name":"Visitor","email":"visitor@example.test","message":"Hello"}),
    )
    .await;
    let id: i64 = sqlx::query_scalar("SELECT MAX(id) FROM feedback")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let path = format!("/api/feedback/{id}/ai/draft");
    let draft=app.ok(&admin,Method::POST,&path,json!({"recipient":"visitor@example.test","subject":"Your message","body":"We received your message.","knowledge_ids":[]})).await;
    assert_eq!(draft["revision"], 1);
    let updated=app.ok(&admin,Method::PUT,&path,json!({"expected_revision":1,"recipient":"visitor@example.test","subject":"Your message","body":"A human reviewed your message.","knowledge_ids":[]})).await;
    assert_eq!(updated["revision"], 2);
    assert_ne!(updated["content_hash"], draft["content_hash"]);
    assert_eq!(app.call(Some(&admin),Method::PUT,&path,Some(json!({"expected_revision":1,"recipient":"visitor@example.test","subject":"Stale","body":"stale","knowledge_ids":[]}))).await.0,StatusCode::CONFLICT);
    let send=app.call(Some(&admin),Method::POST,&format!("/api/feedback/{id}/ai/send"),Some(json!({"expected_revision":2,"content_hash":updated["content_hash"],"idempotency_key":"human-send-0000001"}))).await;
    assert_eq!(send.0, StatusCode::BAD_REQUEST);
    assert!(send.1["error"].as_str().unwrap().contains("SMTP"));
    let queue = app.get(&admin, "/api/ai/review").await;
    assert_eq!(queue["items"][0]["feedback_id"], id);
    let (_, other) = app.member(&admin, "unassigned").await;
    assert_eq!(app.get(&other, "/api/ai/review").await["items"], json!([]));
    assert_eq!(
        app.call(
            Some(&other),
            Method::GET,
            &format!("/api/feedback/{id}/ai"),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

async fn smtp_fixture(
    unknown: bool,
) -> (
    feedback_inbox::config::SmtpConfig,
    std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    tokio::task::JoinHandle<()>,
) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = messages.clone();
    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let seen = seen.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut read = BufReader::new(read);
                write.write_all(b"220 fixture ESMTP\r\n").await.unwrap();
                let mut data = false;
                let mut message = String::new();
                loop {
                    let mut line = String::new();
                    if read.read_line(&mut line).await.unwrap_or(0) == 0 {
                        break;
                    }
                    if data {
                        if line == ".\r\n" {
                            seen.lock().unwrap().push(message.clone());
                            if unknown {
                                break;
                            }
                            write.write_all(b"250 accepted\r\n").await.unwrap();
                            data = false;
                        } else {
                            message.push_str(&line)
                        }
                        continue;
                    }
                    let response = if line.starts_with("EHLO") {
                        b"250-fixture\r\n250-8BITMIME\r\n250 SMTPUTF8\r\n".as_slice()
                    } else if line.starts_with("DATA") {
                        data = true;
                        b"354 Send data\r\n".as_slice()
                    } else if line.starts_with("QUIT") {
                        break;
                    } else {
                        b"250 OK\r\n".as_slice()
                    };
                    if write.write_all(response).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    (
        feedback_inbox::config::SmtpConfig {
            host: "127.0.0.1".into(),
            port,
            username: None,
            password: None,
            from: "Inbox <inbox@example.test>".into(),
            tls: feedback_inbox::config::SmtpTls::None,
        },
        messages,
        server,
    )
}
#[tokio::test]
async fn exact_approval_is_idempotent_and_smtp_ambiguity_is_never_retried() {
    for unknown in [false, true] {
        let (smtp, messages, server) = smtp_fixture(unknown).await;
        let app = TestApp::with_smtp(0, Default::default(), Some(smtp)).await;
        let admin = app.admin().await;
        let site = app
            .site(&admin, json!({"name":"Replies","notify_email":false}))
            .await;
        app.submit(
            site["key"].as_str().unwrap(),
            json!({"name":"Visitor","email":"visitor@example.test","message":"Hello"}),
        )
        .await;
        let id: i64 = sqlx::query_scalar("SELECT MAX(id) FROM feedback")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
        let draft=app.ok(&admin,Method::POST,&format!("/api/feedback/{id}/ai/draft"),json!({"recipient":"visitor@example.test","subject":"Reviewed subject","body":"Reviewed reply body","knowledge_ids":[]})).await;
        let approval = json!({"expected_revision":1,"content_hash":draft["content_hash"],"idempotency_key":"human-send-exact-0001"});
        let path = format!("/api/feedback/{id}/ai/send");
        let sent = app.ok(&admin, Method::POST, &path, approval.clone()).await;
        let duplicate = app.ok(&admin, Method::POST, &path, approval.clone()).await;
        assert_eq!(sent["id"], duplicate["id"]);
        while feedback_inbox::jobs::run_once(&app.state).await.unwrap() {}
        let state = app.get(&admin, &format!("/api/feedback/{id}/ai")).await;
        assert_eq!(
            state["outbound"][0]["state"],
            if unknown { "delivery_unknown" } else { "sent" },
            "{state}"
        );
        assert_eq!(messages.lock().unwrap().len(), 1);
        let message = messages.lock().unwrap()[0].clone();
        assert!(message.contains("Auto-Submitted: auto-replied"));
        assert!(message.contains("visitor@example.test"));
        assert!(message.contains("Reviewed reply body"));
        assert!(!feedback_inbox::jobs::run_once(&app.state).await.unwrap());
        server.abort();
    }
}
#[tokio::test]
async fn revoking_reviewer_cancels_before_transmission() {
    let (smtp, messages, server) = smtp_fixture(false).await;
    let app = TestApp::with_smtp(0, Default::default(), Some(smtp)).await;
    let admin = app.admin().await;
    let (uid, member) = app.member(&admin, "reviewer").await;
    let site = app
        .site(
            &admin,
            json!({"name":"Replies","notify_email":false,"member_ids":[uid]}),
        )
        .await;
    let site_id = site["id"].as_i64().unwrap();
    // Site API uses members; enforce fixture membership explicitly to isolate send authority.
    sqlx::query("INSERT OR IGNORE INTO site_members(site_id,user_id) VALUES(?,?)")
        .bind(site_id)
        .bind(uid)
        .execute(&app.state.db)
        .await
        .unwrap();
    app.submit(
        site["key"].as_str().unwrap(),
        json!({"name":"Visitor","email":"visitor@example.test","message":"Hello"}),
    )
    .await;
    let id: i64 = sqlx::query_scalar("SELECT MAX(id) FROM feedback")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let draft=app.ok(&member,Method::POST,&format!("/api/feedback/{id}/ai/draft"),json!({"recipient":"visitor@example.test","subject":"Review","body":"Reviewed body","knowledge_ids":[]})).await;
    app.ok(&member,Method::POST,&format!("/api/feedback/{id}/ai/send"),json!({"expected_revision":1,"content_hash":draft["content_hash"],"idempotency_key":"human-revoke-0001"})).await;
    sqlx::query("DELETE FROM site_members WHERE site_id=? AND user_id=?")
        .bind(site_id)
        .bind(uid)
        .execute(&app.state.db)
        .await
        .unwrap();
    while feedback_inbox::jobs::run_once(&app.state).await.unwrap() {}
    assert!(messages.lock().unwrap().is_empty());
    let state = app.get(&admin, &format!("/api/feedback/{id}/ai")).await;
    assert_eq!(state["outbound"][0]["state"], "cancelled");
    server.abort();
}

#[tokio::test]
async fn controlled_auto_template_sends_only_verified_consented_future_feedback() {
    use axum::{routing::post, Json, Router};
    use base64::Engine;
    use hmac::Mac;
    use serde_json::Value;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let provider_server = tokio::spawn(async move {
        axum::serve(listener,Router::new().route("/api/chat",post(|Json(b):Json<Value>|async move{let i:Value=serde_json::from_str(b["messages"][1]["content"].as_str().unwrap()).unwrap();let o=if i.get("check").is_some(){json!({"ok":true})}else{let f=&i["fields"][0];let text=f["text"].as_str().unwrap();json!({"summary":"一般詢問","language":"zh-TW","category":i["categories"][0],"tags":[],"urgency":"normal","owner_id":null,"next_action":"收件確認","missing_information":[],"risk_flags":[],"evidence":[{"field_id":f["field_id"],"start":0,"end":text.chars().count(),"quote":text}],"draft":null})};Json(json!({"done":true,"message":{"content":o.to_string()},"model":"fixture"}))}))).await.unwrap()
    });
    let (smtp, messages, smtp_server) = smtp_fixture(false).await;
    let app = TestApp::with_smtp(
        0,
        feedback_inbox::config::WorkflowConfig {
            secret_key: Some(feedback_inbox::config::EncryptionKey([7; 32])),
            allowed_ollama_origins: vec![origin.clone()],
            auto_send_enabled: true,
            ..Default::default()
        },
        Some(smtp),
    )
    .await;
    let admin = app.admin().await;
    let site = app
        .site(&admin, json!({"name":"Auto","notify_email":false}))
        .await;
    let sid = site["id"].as_i64().unwrap();
    let key = site["key"].as_str().unwrap();
    let form_path = format!("/api/sites/{sid}/form");
    let form = app.get(&admin, &form_path).await;
    let mut schema = form["draft"]["schema"].clone();
    let selector = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let consent = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    schema["fields"].as_array_mut().unwrap().extend([json!({"id":selector,"key":"request_type","label":"類型","type":"select","required":true,"sensitivity":"public","options":[{"value":"general","label":"一般詢問"}]}),json!({"id":consent,"key":"reply_requested","label":"我希望收到回覆","type":"checkbox","sensitivity":"public"})]);
    app.ok(
        &admin,
        Method::PUT,
        &format!("{form_path}/draft"),
        json!({"expected_revision":1,"schema":schema}),
    )
    .await;
    let version = app
        .ok(
            &admin,
            Method::POST,
            &format!("{form_path}/publish"),
            json!({"expected_revision":2}),
        )
        .await["version"]
        .clone();
    let provider=app.ok(&admin,Method::POST,"/api/ai/providers",json!({"name":"Fixture","kind":"ollama","base_url":origin,"model":"fixture","enabled":true})).await;
    let pid = provider["id"].as_i64().unwrap();
    app.ok(
        &admin,
        Method::POST,
        &format!("/api/ai/providers/{pid}/test"),
        json!({"expected_revision":1}),
    )
    .await;
    let tpl_path = format!("/api/sites/{sid}/ai/templates");
    let tpl=app.ok(&admin,Method::POST,&tpl_path,json!({"name":"收件確認","subject":"[{site_name}] 已收到您的訊息","body":"我們已收到您的訊息（編號 {reference}），工作人員會盡快回覆。","knowledge_ids":[]})).await;
    let tid = tpl["id"].as_i64().unwrap();
    app.ok(
        &admin,
        Method::POST,
        &format!("{tpl_path}/{tid}/approve"),
        json!({"expected_revision":1}),
    )
    .await;
    let context = app.get(&admin, &format!("/api/sites/{sid}/ai")).await;
    let category = context["settings"]["categories"][0].clone();
    app.ok(&admin,Method::POST,&format!("/api/sites/{sid}/ai/rules"),json!({"name":"General acknowledgement","enabled":true,"field_id":selector,"equals":"general","category":category,"template_id":tid})).await;
    let context = app.get(&admin, &format!("/api/sites/{sid}/ai")).await;
    let field = context["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == "message")
        .unwrap()["id"]
        .clone();
    let mut policy = context["settings"].clone();
    policy["expected_revision"] = policy["revision"].clone();
    policy["enabled"] = json!(true);
    policy["provider_id"] = json!(pid);
    policy["field_ids"] = json!([field, selector]);
    policy["generate_on_receipt"] = json!(true);
    policy["auto_mode"] = json!("auto_template");
    app.ok(&admin, Method::PUT, &format!("/api/sites/{sid}/ai"), policy)
        .await;
    let signing = app
        .ok(
            &admin,
            Method::POST,
            &format!("/api/sites/{sid}/identity-key/rotate"),
            json!({}),
        )
        .await;
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(signing["secret"].as_str().unwrap())
        .unwrap();
    let token = |email: &str, jti: &str| {
        let now = chrono::Utc::now().timestamp();
        let claims = json!({"v":1,"site_id":sid,"sub":"member-1","email":email,"email_verified":true,"iat":now,"exp":now+300,"jti":jti});
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string());
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(&secret).unwrap();
        mac.update(payload.as_bytes());
        format!(
            "{payload}.{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        )
    };
    for (index, verified, consented, expected) in [
        (1, true, true, "sent"),
        (2, false, true, "recipient_unverified"),
        (3, true, false, "consent_missing"),
        (4, true, true, "recipient_quota"),
    ] {
        let mut body = json!({"_form_version":version,"name":"Visitor","email":"member@example.test","message":"Hello","request_type":"general","reply_requested":consented});
        if verified {
            body["_identity_token"] = json!(token(
                "member@example.test",
                &format!("unique-identity-{index:04}")
            ));
        }
        let (status, result) = app.submit(key, body).await;
        assert_eq!(status, StatusCode::OK, "{result}");
        let fid: i64 = sqlx::query_scalar("SELECT MAX(id) FROM feedback")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
        while feedback_inbox::jobs::run_once(&app.state).await.unwrap() {}
        let result = app.get(&admin, &format!("/api/feedback/{fid}/ai")).await;
        assert_eq!(result["runs"][0]["state"], "succeeded", "{result}");
        if expected == "sent" {
            assert_eq!(result["outbound"][0]["state"], "sent", "{result}");
            assert_eq!(result["outbound"][0]["mode"], "auto_template");
        } else {
            assert_eq!(result["auto_decision"]["gate"], expected, "{result}");
            assert_eq!(result["outbound"], json!([]));
        }
    }
    assert_eq!(messages.lock().unwrap().len(), 1);
    // Pausing wins atomically after analysis reserves a reply but before SMTP begins.
    let body = json!({"_form_version":version,"name":"Visitor","email":"second@example.test","message":"Hello","request_type":"general","reply_requested":true,"_identity_token":token("second@example.test","unique-identity-pause")});
    assert_eq!(app.submit(key, body).await.0, StatusCode::OK);
    let paused_id: i64 = sqlx::query_scalar("SELECT MAX(id) FROM feedback")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert!(feedback_inbox::jobs::run_once(&app.state).await.unwrap());
    assert_eq!(
        app.get(&admin, &format!("/api/feedback/{paused_id}/ai"))
            .await["outbound"][0]["state"],
        "queued"
    );
    let context = app.get(&admin, &format!("/api/sites/{sid}/ai")).await;
    let mut policy = context["settings"].clone();
    policy["expected_revision"] = policy["revision"].clone();
    policy["auto_mode"] = json!("off");
    app.ok(&admin, Method::PUT, &format!("/api/sites/{sid}/ai"), policy)
        .await;
    while feedback_inbox::jobs::run_once(&app.state).await.unwrap() {}
    assert_eq!(
        app.get(&admin, &format!("/api/feedback/{paused_id}/ai"))
            .await["outbound"][0]["state"],
        "cancelled"
    );
    assert_eq!(messages.lock().unwrap().len(), 1);
    // Resuming takes a new high-water mark, so it cannot resurrect paused historical work.
    let context = app.get(&admin, &format!("/api/sites/{sid}/ai")).await;
    let mut policy = context["settings"].clone();
    policy["expected_revision"] = policy["revision"].clone();
    policy["auto_mode"] = json!("auto_template");
    app.ok(&admin, Method::PUT, &format!("/api/sites/{sid}/ai"), policy)
        .await;
    assert_eq!(
        app.ok(
            &admin,
            Method::POST,
            &format!("/api/sites/{sid}/ai/auto/dry-run"),
            json!({"feedback_id":paused_id})
        )
        .await["gate"],
        "before_activation"
    );
    // A template retirement cancels a newly reserved reply before delivery.
    let body = json!({"_form_version":version,"name":"Visitor","email":"third@example.test","message":"Hello","request_type":"general","reply_requested":true,"_identity_token":token("third@example.test","unique-identity-retire")});
    assert_eq!(app.submit(key, body).await.0, StatusCode::OK);
    let retired_id: i64 = sqlx::query_scalar("SELECT MAX(id) FROM feedback")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert!(feedback_inbox::jobs::run_once(&app.state).await.unwrap());
    assert_eq!(
        app.get(&admin, &format!("/api/feedback/{retired_id}/ai"))
            .await["outbound"][0]["state"],
        "queued"
    );
    app.ok(
        &admin,
        Method::POST,
        &format!("{tpl_path}/{tid}/retire"),
        json!({"expected_revision":1}),
    )
    .await;
    while feedback_inbox::jobs::run_once(&app.state).await.unwrap() {}
    assert_eq!(
        app.get(&admin, &format!("/api/feedback/{retired_id}/ai"))
            .await["outbound"][0]["state"],
        "cancelled"
    );
    assert_eq!(messages.lock().unwrap().len(), 1);
    smtp_server.abort();
    provider_server.abort();
}

#[tokio::test]
async fn lost_reply_claim_retries_only_before_the_transmission_boundary() {
    for transmission_started in [false, true] {
        let (smtp, messages, server) = smtp_fixture(false).await;
        let app = TestApp::with_smtp(0, Default::default(), Some(smtp)).await;
        let admin = app.admin().await;
        let site = app
            .site(&admin, json!({"name":"Recovery","notify_email":false}))
            .await;
        app.submit(
            site["key"].as_str().unwrap(),
            json!({"name":"Visitor","email":"recovery@example.test","message":"Recover safely"}),
        )
        .await;
        let fid: i64 = sqlx::query_scalar("SELECT MAX(id) FROM feedback")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
        let draft = app.ok(&admin, Method::POST, &format!("/api/feedback/{fid}/ai/draft"), json!({"recipient":"recovery@example.test","subject":"Reviewed","body":"Reviewed recovery reply","knowledge_ids":[]})).await;
        let outbound = app.ok(&admin, Method::POST, &format!("/api/feedback/{fid}/ai/send"), json!({"expected_revision":1,"content_hash":draft["content_hash"],"idempotency_key":"recovery-send-0001"})).await;
        let now = chrono::Utc::now().timestamp();
        let claim = feedback_inbox::jobs::claim(&app.state.db, now)
            .await
            .unwrap()
            .unwrap();
        if transmission_started {
            sqlx::query("UPDATE outbound_replies SET state='sending' WHERE id=?")
                .bind(outbound["id"].as_i64().unwrap())
                .execute(&app.state.db)
                .await
                .unwrap();
        }
        sqlx::query("UPDATE workflow_jobs SET lease_until=? WHERE id=?")
            .bind(now - 1)
            .bind(claim.id)
            .execute(&app.state.db)
            .await
            .unwrap();
        assert!(feedback_inbox::jobs::claim(&app.state.db, now)
            .await
            .unwrap()
            .is_none());
        let state = app.get(&admin, &format!("/api/feedback/{fid}/ai")).await;
        assert_eq!(
            state["outbound"][0]["state"],
            if transmission_started {
                "delivery_unknown"
            } else {
                "queued"
            }
        );
        let job_state: String = sqlx::query_scalar("SELECT state FROM workflow_jobs WHERE id=?")
            .bind(claim.id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
        assert_eq!(
            job_state,
            if transmission_started {
                "delivery_unknown"
            } else {
                "queued"
            }
        );
        sqlx::query("UPDATE workflow_jobs SET available_at=0 WHERE id=?")
            .bind(claim.id)
            .execute(&app.state.db)
            .await
            .unwrap();
        while feedback_inbox::jobs::run_once(&app.state).await.unwrap() {}
        assert_eq!(
            messages.lock().unwrap().len(),
            if transmission_started { 0 } else { 1 }
        );
        assert!(!feedback_inbox::jobs::heartbeat(&app.state.db, &claim)
            .await
            .unwrap());
        server.abort();
    }
}
