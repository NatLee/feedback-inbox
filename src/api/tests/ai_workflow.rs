mod support;
use feedback_inbox::jobs;
use serde_json::json;
use support::TestApp;
#[tokio::test]
async fn two_workers_claim_once_and_cancelled_worker_cannot_finish() {
    let app = TestApp::new().await;
    let admin = app.admin().await;
    let site = app.site(&admin, json!({"name":"Durable"})).await;
    let site = site["id"].as_i64().unwrap();
    let mut tx = app.state.db.begin().await.unwrap();
    let id = jobs::enqueue(&mut tx, site, "analysis", json!({}), "unique")
        .await
        .unwrap();
    assert_eq!(
        jobs::enqueue(&mut tx, site, "analysis", json!({}), "unique")
            .await
            .unwrap(),
        id
    );
    tx.commit().await.unwrap();
    let now = chrono::Utc::now().timestamp();
    let (a, b) = tokio::join!(
        jobs::claim(&app.state.db, now),
        jobs::claim(&app.state.db, now)
    );
    let claims: Vec<_> = [a.unwrap(), b.unwrap()].into_iter().flatten().collect();
    assert_eq!(claims.len(), 1);
    assert!(jobs::cancel(&app.state.db, id).await.unwrap());
    assert!(
        !jobs::finish(&app.state.db, &claims[0], "succeeded", None, None)
            .await
            .unwrap()
    );
    assert!(jobs::claim(&app.state.db, now + 1000)
        .await
        .unwrap()
        .is_none());
}
#[tokio::test]
async fn rollback_and_unknown_delivery_are_not_retried() {
    let app = TestApp::new().await;
    let admin = app.admin().await;
    let site = app.site(&admin, json!({"name":"Durable"})).await;
    let site = site["id"].as_i64().unwrap();
    let mut tx = app.state.db.begin().await.unwrap();
    jobs::enqueue(&mut tx, site, "analysis", json!({}), "rolledback")
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert!(jobs::claim(&app.state.db, chrono::Utc::now().timestamp())
        .await
        .unwrap()
        .is_none());
    let mut tx = app.state.db.begin().await.unwrap();
    jobs::enqueue(&mut tx, site, "reply", json!({}), "smtp")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let now = chrono::Utc::now().timestamp();
    let claim = jobs::claim(&app.state.db, now).await.unwrap().unwrap();
    assert!(jobs::claim(&app.state.db, now + 181)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM workflow_jobs WHERE id=?")
            .bind(claim.id)
            .fetch_one(&app.state.db)
            .await
            .unwrap(),
        "delivery_unknown"
    );
    assert!(
        !jobs::finish(&app.state.db, &claim, "succeeded", None, None)
            .await
            .unwrap()
    );
}
#[tokio::test]
async fn provider_secrets_never_roundtrip_and_destinations_are_allowlisted() {
    use axum::http::{Method, StatusCode};
    let app = TestApp::new().await;
    let admin = app.admin().await;
    let (_, member) = app.member(&admin, "worker").await;
    let body = json!({"name":"Local","kind":"ollama","base_url":"http://127.0.0.1:11434","model":"fixture","enabled":false,"credential":"never-return-this-secret"});
    assert_eq!(
        app.call(
            Some(&member),
            Method::POST,
            "/api/ai/providers",
            Some(body.clone())
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let created = app
        .ok(&admin, Method::POST, "/api/ai/providers", body.clone())
        .await;
    assert!(created["has_credential"].as_bool().unwrap());
    assert!(!created.to_string().contains("never-return"));
    let list = app.get(&admin, "/api/ai/providers").await;
    assert!(!list.to_string().contains("never-return"));
    let stored: String =
        sqlx::query_scalar("SELECT credential_ciphertext FROM ai_providers WHERE id=?")
            .bind(created["id"].as_i64().unwrap())
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(!stored.contains("never-return"));
    for url in [
        "http://169.254.169.254/",
        "http://127.0.0.1:11434/path",
        "http://user:secret@127.0.0.1:11434/",
        "http://127.0.0.1:11434/?token=x",
    ] {
        let mut invalid = body.clone();
        invalid["base_url"] = json!(url);
        assert_eq!(
            app.call(
                Some(&admin),
                Method::POST,
                "/api/ai/providers",
                Some(invalid)
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let mut invalid = body;
    invalid["kind"] = json!("openai");
    invalid["base_url"] = json!("https://api.openai.com.evil.example");
    assert_eq!(
        app.call(
            Some(&admin),
            Method::POST,
            "/api/ai/providers",
            Some(invalid)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}
#[tokio::test]
async fn policy_defaults_are_off_and_knowledge_edits_invalidate_approval() {
    use axum::http::{Method, StatusCode};
    let app = TestApp::new().await;
    let admin = app.admin().await;
    let site = app.site(&admin, json!({"name":"Policy"})).await;
    let id = site["id"].as_i64().unwrap();
    let context = app.get(&admin, &format!("/api/sites/{id}/ai")).await;
    assert_eq!(context["settings"]["enabled"], false);
    assert_eq!(context["settings"]["field_ids"], json!([]));
    assert_eq!(context["settings"]["auto_mode"], "off");
    let mut policy = context["settings"].clone();
    policy["expected_revision"] = policy["revision"].clone();
    policy["daily_attempt_limit"] = json!(1001);
    assert_eq!(
        app.call(
            Some(&admin),
            Method::PUT,
            &format!("/api/sites/{id}/ai"),
            Some(policy)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let path = format!("/api/sites/{id}/ai/knowledge");
    let k = app
        .ok(
            &admin,
            Method::POST,
            &path,
            json!({"title":"Hours","body":"Monday–Friday 09:00–17:00"}),
        )
        .await;
    let kid = k["id"].as_i64().unwrap();
    let approved = app
        .ok(
            &admin,
            Method::POST,
            &format!("{path}/{kid}/approve"),
            json!({"expected_revision":1}),
        )
        .await;
    assert_eq!(approved["state"], "approved");
    let edited = app
        .ok(
            &admin,
            Method::PUT,
            &format!("{path}/{kid}"),
            json!({"expected_revision":1,"title":"Hours","body":"Monday–Friday 10:00–17:00"}),
        )
        .await;
    assert_ne!(edited["id"], kid);
    assert_eq!(edited["revision"], 2);
    assert_eq!(edited["state"], "draft");
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT body FROM ai_knowledge WHERE id=?")
            .bind(kid)
            .fetch_one(&app.state.db)
            .await
            .unwrap(),
        "Monday–Friday 09:00–17:00"
    );
    let mut tx = app.state.db.begin().await.unwrap();
    assert!(
        feedback_inbox::ai::policy::knowledge_valid(&mut tx, id, &[kid])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn local_provider_analysis_redacts_fields_and_preserves_workflow_status() {
    use axum::{http::Method, routing::post, Json, Router};
    use serde_json::Value;
    use std::sync::{Arc, Mutex};
    let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
    let seen = captured.clone();
    let fixture=Router::new().route("/api/chat",post(move |Json(body):Json<Value>|{let seen=seen.clone();async move{let input:Value=serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();seen.lock().unwrap().push(input.clone());let output=if input.get("check").is_some(){json!({"ok":true})}else{let field=&input["fields"][0];let text=field["text"].as_str().unwrap();json!({"summary":"營業時間詢問","language":"zh-TW","category":input["categories"][0],"tags":[],"urgency":"normal","owner_id":null,"next_action":"回覆核准營業時間","missing_information":[],"risk_flags":[],"evidence":[{"field_id":field["field_id"],"start":0,"end":text.chars().count(),"quote":text}],"draft":null})};Json(json!({"model":"fixture","done":true,"done_reason":"stop","message":{"content":output.to_string()},"prompt_eval_count":10,"eval_count":10}))}}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, fixture).await.unwrap() });
    let app = TestApp::with_workflow(
        0,
        feedback_inbox::config::WorkflowConfig {
            secret_key: Some(feedback_inbox::config::EncryptionKey([7; 32])),
            allowed_ollama_origins: vec![origin.clone()],
            ..Default::default()
        },
    )
    .await;
    let admin = app.admin().await;
    let site = app.site(&admin, json!({"name":"Analysis"})).await;
    let id = site["id"].as_i64().unwrap();
    let provider=app.ok(&admin,Method::POST,"/api/ai/providers",json!({"name":"Fixture","kind":"ollama","base_url":origin,"model":"fixture","enabled":true})).await;
    let pid = provider["id"].as_i64().unwrap();
    let tested = app
        .ok(
            &admin,
            Method::POST,
            &format!("/api/ai/providers/{pid}/test"),
            json!({"expected_revision":1}),
        )
        .await;
    assert_eq!(tested["tested_revision"], 1);
    let context = app.get(&admin, &format!("/api/sites/{id}/ai")).await;
    let field = context["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == "message")
        .unwrap();
    let field_id = field["id"].clone();
    let mut settings = context["settings"].clone();
    settings["expected_revision"] = settings["revision"].clone();
    settings["enabled"] = json!(true);
    settings["provider_id"] = json!(pid);
    settings["field_ids"] = json!([field_id]);
    app.ok(
        &admin,
        Method::PUT,
        &format!("/api/sites/{id}/ai"),
        settings,
    )
    .await;
    app.submit(site["key"].as_str().unwrap(),json!({"name":"Sensitive Name","email":"private@example.com","message":"😀 請問營業時間？找 private@example.com 或 +886 912 345 678"})).await;
    let fid: i64 = sqlx::query_scalar("SELECT MAX(id) FROM feedback")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    app.ok(
        &admin,
        Method::POST,
        &format!("/api/feedback/{fid}/ai/analyze"),
        json!({}),
    )
    .await;
    while jobs::run_once(&app.state).await.unwrap() {}
    let details = app.get(&admin, &format!("/api/feedback/{fid}/ai")).await;
    assert_eq!(details["runs"][0]["state"], "succeeded", "{details}");
    assert_eq!(details["proposal"]["state"], "pending");
    assert_eq!(
        app.get(&admin, &format!("/api/feedback/{fid}")).await["status"],
        "new"
    );
    let input = captured.lock().unwrap().last().unwrap().to_string();
    assert!(!input.contains("private@example.com"));
    assert!(!input.contains("Sensitive Name"));
    assert!(!input.contains("912 345 678"));
    assert!(input.contains("[EMAIL]"));
    assert!(!input.contains("identity"));
    server.abort();
}

#[tokio::test]
async fn expired_claim_cannot_heartbeat_or_finish_and_running_analysis_has_global_bound() {
    let app = TestApp::new().await;
    let admin = app.admin().await;
    let site = app.site(&admin, json!({"name":"Lease"})).await;
    let site = site["id"].as_i64().unwrap();
    let mut tx = app.state.db.begin().await.unwrap();
    for i in 0..4 {
        jobs::enqueue(&mut tx, site, "analysis", json!({}), &format!("lease-{i}"))
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    let now = chrono::Utc::now().timestamp();
    let first = jobs::claim(&app.state.db, now).await.unwrap().unwrap();
    let _second = jobs::claim(&app.state.db, now).await.unwrap().unwrap();
    assert!(
        jobs::claim(&app.state.db, now).await.unwrap().is_none(),
        "global analysis concurrency must be bounded across independent worker claims"
    );
    sqlx::query("UPDATE workflow_jobs SET lease_until=? WHERE id=?")
        .bind(now - 1)
        .bind(first.id)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert!(!jobs::heartbeat(&app.state.db, &first).await.unwrap());
    assert!(
        !jobs::finish(&app.state.db, &first, "succeeded", None, None)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn encryption_rotation_is_atomic_and_invalidates_provider_verification() {
    use axum::http::Method;
    use feedback_inbox::{
        ai::secrets::{self, SecretStore},
        config::EncryptionKey,
    };
    let app = TestApp::new().await;
    let admin = app.admin().await;
    let site = app.site(&admin, json!({"name":"Rotation"})).await;
    let sid = site["id"].as_i64().unwrap();
    let p = app.ok(&admin, Method::POST, "/api/ai/providers", json!({"name":"Credential fixture","kind":"ollama","base_url":"http://127.0.0.1:11434","model":"fixture","enabled":false,"credential":"rotation-fixture-secret"})).await;
    let pid = p["id"].as_i64().unwrap();
    let identity = app
        .ok(
            &admin,
            Method::POST,
            &format!("/api/sites/{sid}/identity-key/rotate"),
            json!({}),
        )
        .await;
    let before: String =
        sqlx::query_scalar("SELECT credential_ciphertext FROM ai_providers WHERE id=?")
            .bind(pid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    let identity_before: String =
        sqlx::query_scalar("SELECT ciphertext FROM identity_keys WHERE site_id=?")
            .bind(sid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    sqlx::query("UPDATE identity_keys SET ciphertext='corrupted fixture' WHERE site_id=?")
        .bind(sid)
        .execute(&app.state.db)
        .await
        .unwrap();
    let next = EncryptionKey([9; 32]);
    assert!(secrets::reencrypt(&app.state, &next).await.is_err());
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT credential_ciphertext FROM ai_providers WHERE id=?"
        )
        .bind(pid)
        .fetch_one(&app.state.db)
        .await
        .unwrap(),
        before
    );
    sqlx::query("UPDATE identity_keys SET ciphertext=? WHERE site_id=?")
        .bind(identity_before)
        .bind(sid)
        .execute(&app.state.db)
        .await
        .unwrap();
    let mut tx = app.state.db.begin().await.unwrap();
    let queued = jobs::enqueue(&mut tx, sid, "analysis", json!({}), "rotate-queued")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let claim = jobs::claim(&app.state.db, chrono::Utc::now().timestamp())
        .await
        .unwrap()
        .unwrap();
    assert!(
        secrets::reencrypt(&app.state, &next).await.is_err(),
        "active leases block offline rotation"
    );
    assert!(jobs::cancel(&app.state.db, claim.id).await.unwrap());
    sqlx::query("UPDATE workflow_jobs SET state='queued' WHERE id=?")
        .bind(queued)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(secrets::reencrypt(&app.state, &next).await.unwrap(), 2);
    let after: String =
        sqlx::query_scalar("SELECT credential_ciphertext FROM ai_providers WHERE id=?")
            .bind(pid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(secrets::for_state(&app.state)
        .unwrap()
        .decrypt("provider", pid, &after)
        .is_err());
    assert_eq!(
        SecretStore::new(&next)
            .decrypt("provider", pid, &after)
            .unwrap(),
        "rotation-fixture-secret"
    );
    let ciphertext: String =
        sqlx::query_scalar("SELECT ciphertext FROM identity_keys WHERE site_id=?")
            .bind(sid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(
        SecretStore::new(&next)
            .decrypt("identity", sid, &ciphertext)
            .unwrap(),
        identity["secret"].as_str().unwrap()
    );
    let updated = app.get(&admin, "/api/ai/providers").await;
    assert_eq!(updated["items"][0]["revision"], 2);
    assert!(updated["items"][0]["tested_revision"].is_null());
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM workflow_jobs WHERE id=?")
            .bind(queued)
            .fetch_one(&app.state.db)
            .await
            .unwrap(),
        "cancelled"
    );
}

#[tokio::test]
async fn changing_authority_policy_source_knowledge_or_provider_fences_model_egress_and_results() {
    use axum::{http::Method, routing::post, Json, Router};
    use serde_json::Value;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    for inflight in [false, true] {
        for mutation in [
            "authority",
            "policy",
            "source",
            "knowledge",
            "provider",
            "field_permission",
            "account_status",
        ] {
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let calls = Arc::new(AtomicUsize::new(0));
            let (seen, gate, count) = (entered.clone(), release.clone(), calls.clone());
            let fixture = Router::new().route("/api/chat", post(move |Json(body):Json<Value>| {
                let (seen, gate, count) = (seen.clone(), gate.clone(), count.clone());
                async move {
                    let input: Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
                    let output = if input.get("check").is_some() { json!({"ok":true}) } else {
                        count.fetch_add(1, Ordering::SeqCst); seen.notify_one(); if inflight { gate.notified().await; }
                        let f = &input["fields"][0]; let text = f["text"].as_str().unwrap();
                        json!({"summary":"Fixture result","language":"en","category":input["categories"][0],"tags":[],"urgency":"normal","owner_id":null,"next_action":"Human review","missing_information":[],"risk_flags":[],"evidence":[{"field_id":f["field_id"],"start":0,"end":text.chars().count(),"quote":text}],"draft":{"subject":"Approved information","body":"Hours: weekdays.","knowledge_ids":[input["knowledge"][0]["id"]]}})
                    };
                    Json(json!({"done":true,"message":{"content":output.to_string()},"model":"fixture"}))
                }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, fixture).await.unwrap() });
            let app = TestApp::with_workflow(
                0,
                feedback_inbox::config::WorkflowConfig {
                    secret_key: Some(feedback_inbox::config::EncryptionKey([7; 32])),
                    allowed_ollama_origins: vec![origin.clone()],
                    ..Default::default()
                },
            )
            .await;
            let admin = app.admin().await;
            let (uid, member) = app.member(&admin, "race-reviewer").await;
            let site = app
                .site(
                    &admin,
                    json!({"name":"Fencing","notify_email":false,"member_ids":[uid]}),
                )
                .await;
            let sid = site["id"].as_i64().unwrap();
            let provider = app.ok(&admin, Method::POST, "/api/ai/providers", json!({"name":"Fixture","kind":"ollama","base_url":origin,"model":"fixture","enabled":true})).await;
            let pid = provider["id"].as_i64().unwrap();
            app.ok(
                &admin,
                Method::POST,
                &format!("/api/ai/providers/{pid}/test"),
                json!({"expected_revision":1}),
            )
            .await;
            let kp = format!("/api/sites/{sid}/ai/knowledge");
            let k = app
                .ok(
                    &admin,
                    Method::POST,
                    &kp,
                    json!({"title":"Hours","body":"Hours: weekdays."}),
                )
                .await;
            let kid = k["id"].as_i64().unwrap();
            app.ok(
                &admin,
                Method::POST,
                &format!("{kp}/{kid}/approve"),
                json!({"expected_revision":1}),
            )
            .await;
            let context = app.get(&admin, &format!("/api/sites/{sid}/ai")).await;
            let field = context["fields"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["key"] == "message")
                .unwrap()["id"]
                .clone();
            let mut settings = context["settings"].clone();
            settings["expected_revision"] = settings["revision"].clone();
            settings["enabled"] = json!(true);
            settings["provider_id"] = json!(pid);
            settings["field_ids"] = json!([field]);
            app.ok(
                &admin,
                Method::PUT,
                &format!("/api/sites/{sid}/ai"),
                settings,
            )
            .await;
            app.submit(
                site["key"].as_str().unwrap(),
                json!({"name":"Visitor","email":"race@example.test","message":"😀 Hours please"}),
            )
            .await;
            let fid: i64 = sqlx::query_scalar("SELECT MAX(id) FROM feedback")
                .fetch_one(&app.state.db)
                .await
                .unwrap();
            app.ok(
                &member,
                Method::POST,
                &format!("/api/feedback/{fid}/ai/analyze"),
                json!({}),
            )
            .await;
            let state = app.state.clone();
            let worker = if inflight {
                let worker = tokio::spawn(async move { jobs::run_once(&state).await.unwrap() });
                tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
                    .await
                    .unwrap();
                Some(worker)
            } else {
                None
            };
            match mutation {
                "authority" => {
                    sqlx::query("DELETE FROM site_members WHERE site_id=? AND user_id=?")
                        .bind(sid)
                        .bind(uid)
                        .execute(&app.state.db)
                        .await
                        .unwrap();
                }
                "policy" => {
                    sqlx::query("UPDATE site_ai_settings SET revision=revision+1 WHERE site_id=?")
                        .bind(sid)
                        .execute(&app.state.db)
                        .await
                        .unwrap();
                }
                "source" => {
                    sqlx::query("UPDATE feedback SET message='Changed source' WHERE id=?")
                        .bind(fid)
                        .execute(&app.state.db)
                        .await
                        .unwrap();
                }
                "knowledge" => {
                    sqlx::query("UPDATE ai_knowledge SET state='retired' WHERE id=?")
                        .bind(kid)
                        .execute(&app.state.db)
                        .await
                        .unwrap();
                }
                "account_status" => {
                    sqlx::query("UPDATE users SET account_status='pending' WHERE id=?")
                        .bind(uid)
                        .execute(&app.state.db)
                        .await
                        .unwrap();
                }
                "field_permission" => {
                    let form_path = format!("/api/sites/{sid}/form");
                    let form = app.get(&admin, &form_path).await;
                    let mut schema = form["draft"]["schema"].clone();
                    let message = schema["fields"]
                        .as_array_mut()
                        .unwrap()
                        .iter_mut()
                        .find(|f| f["key"] == "message")
                        .unwrap();
                    message["sensitivity"] = json!("sensitive");
                    let saved = app
                        .ok(
                            &admin,
                            Method::PUT,
                            &format!("{form_path}/draft"),
                            json!({"expected_revision":form["draft"]["revision"],"schema":schema}),
                        )
                        .await;
                    app.ok(
                        &admin,
                        Method::POST,
                        &format!("{form_path}/publish"),
                        json!({"expected_revision":saved["revision"]}),
                    )
                    .await;
                }
                "provider" => {
                    sqlx::query("UPDATE ai_providers SET enabled=0,revision=revision+1 WHERE id=?")
                        .bind(pid)
                        .execute(&app.state.db)
                        .await
                        .unwrap();
                }
                _ => unreachable!(),
            }
            if let Some(worker) = worker {
                release.notify_one();
                worker.await.unwrap();
            } else {
                assert!(jobs::run_once(&app.state).await.unwrap());
            }
            let result = app.get(&admin, &format!("/api/feedback/{fid}/ai")).await;
            assert_eq!(
                result["runs"][0]["state"], "superseded",
                "phase={inflight}, mutation={mutation}: {result}"
            );
            assert!(result["proposal"].is_null() && result["draft"].is_null());
            assert_eq!(result["outbound"], json!([]));
            assert_eq!(calls.load(Ordering::SeqCst), usize::from(inflight));
            server.abort();
        }
    }
}

#[tokio::test]
async fn inactive_registration_cannot_receive_staff_content_or_authorize_knowledge() {
    use axum::http::Method;
    let app = TestApp::new().await;
    let admin = app.admin().await;
    let (uid, member) = app.member(&admin, "pending-staff").await;
    app.ok(
        &member,
        Method::PATCH,
        "/api/auth/me",
        json!({"email":"pending-staff@example.test"}),
    )
    .await;
    let site = app
        .site(
            &admin,
            json!({"name":"Active staff","member_ids":[uid],"notify_email":true}),
        )
        .await;
    let sid = site["id"].as_i64().unwrap();
    assert_eq!(
        feedback_inbox::notify::targets(&app.state, sid)
            .await
            .unwrap()
            .emails,
        vec!["pending-staff@example.test"]
    );
    let k = app
        .ok(
            &admin,
            Method::POST,
            &format!("/api/sites/{sid}/ai/knowledge"),
            json!({"title":"Policy","body":"Approved fact"}),
        )
        .await;
    let kid = k["id"].as_i64().unwrap();
    app.ok(
        &admin,
        Method::POST,
        &format!("/api/sites/{sid}/ai/knowledge/{kid}/approve"),
        json!({"expected_revision":1}),
    )
    .await;
    sqlx::query("UPDATE users SET account_status='pending' WHERE id=?")
        .bind(uid)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert!(feedback_inbox::notify::targets(&app.state, sid)
        .await
        .unwrap()
        .emails
        .is_empty());
    let context = app.get(&admin, &format!("/api/sites/{sid}/ai")).await;
    assert!(!context["members"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["id"] == uid));
    let aid = app.get(&admin, "/api/auth/state").await["user"]["id"]
        .as_i64()
        .unwrap();
    sqlx::query("UPDATE users SET account_status='rejected' WHERE id=?")
        .bind(aid)
        .execute(&app.state.db)
        .await
        .unwrap();
    let mut tx = app.state.db.begin().await.unwrap();
    assert!(
        feedback_inbox::ai::authority(&mut tx, uid, Some(sid), false)
            .await
            .is_err()
    );
    assert!(
        feedback_inbox::ai::policy::knowledge_valid(&mut tx, sid, &[kid])
            .await
            .is_err()
    );
}
