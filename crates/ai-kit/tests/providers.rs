use ai_kit::*;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    response::IntoResponse,
    routing::any,
    Router,
};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone)]
struct Fixture {
    body: String,
    status: StatusCode,
    delay: u64,
    captured: Arc<Mutex<Vec<(String, HeaderMap, Value)>>>,
}
async fn handler(
    State(s): State<Fixture>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    s.captured.lock().unwrap().push((
        uri.path().into(),
        headers,
        serde_json::from_slice(&body).unwrap_or(Value::Null),
    ));
    tokio::time::sleep(Duration::from_millis(s.delay)).await;
    (
        s.status,
        [("retry-after", "7"), ("x-request-id", "fixture-request")],
        s.body,
    )
}
async fn fixture(
    body: Value,
    status: StatusCode,
    delay: u64,
) -> (url::Url, Fixture, tokio::task::JoinHandle<()>) {
    let s = Fixture {
        body: body.to_string(),
        status,
        delay,
        captured: Default::default(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let router = Router::new().fallback(any(handler)).with_state(s.clone());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (url, s, task)
}
fn request() -> CompletionRequest {
    CompletionRequest {
        contract_version: 1,
        system: "Extract only supplied facts".into(),
        input: json!({"message":"測試"}),
        output_schema: json!({"type":"object","properties":{"category":{"type":"string","enum":["support"]}},"required":["category"],"additionalProperties":false}),
        schema_name: "triage".into(),
        max_output_tokens: 128,
        timeout_ms: 1000,
        request_id: "test-1".into(),
    }
}
fn success(kind: ProviderKind, content: &str) -> Value {
    match kind {
        ProviderKind::OpenAi => {
            json!({"id":"response-1","model":"fixture-v1","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":content}]}],"usage":{"input_tokens":12,"output_tokens":5}})
        }
        ProviderKind::Anthropic => {
            json!({"id":"message-1","model":"fixture-v1","stop_reason":"end_turn","content":[{"type":"text","text":content}],"usage":{"input_tokens":12,"output_tokens":5}})
        }
        ProviderKind::Ollama => {
            json!({"model":"fixture-v1","done":true,"done_reason":"stop","message":{"role":"assistant","content":content},"prompt_eval_count":12,"eval_count":5})
        }
    }
}
#[tokio::test]
async fn providers_send_correct_contract_and_validate_usage() {
    for (kind, path) in [
        (ProviderKind::OpenAi, "/v1/responses"),
        (ProviderKind::Anthropic, "/v1/messages"),
        (ProviderKind::Ollama, "/api/chat"),
    ] {
        let (url, s, task) = fixture(
            success(kind, r#"{"category":"support"}"#),
            StatusCode::OK,
            0,
        )
        .await;
        let client = AiClient::new(
            ProviderConfig {
                kind,
                base_url: url,
                model: "configured-model".into(),
            },
            Some(SecretString::new("private-key")),
        )
        .unwrap();
        let r = client.complete(request()).await.unwrap();
        assert_eq!(r.output, json!({"category":"support"}));
        assert_eq!(r.resolved_model, "fixture-v1");
        assert_eq!(
            r.usage.unwrap(),
            TokenUsage {
                input_tokens: Some(12),
                output_tokens: Some(5)
            }
        );
        let capture = s.captured.lock().unwrap();
        let (actual_path, headers, payload) = &capture[0];
        assert_eq!(actual_path, path);
        assert_eq!(payload["model"], "configured-model");
        assert!(payload.get("tools").is_none());
        match kind {
            ProviderKind::OpenAi => {
                assert_eq!(headers["authorization"], "Bearer private-key");
                assert_eq!(payload["text"]["format"]["type"], "json_schema");
                assert_eq!(payload["max_output_tokens"], 128);
            }
            ProviderKind::Anthropic => {
                assert_eq!(headers["x-api-key"], "private-key");
                assert_eq!(payload["output_config"]["format"]["type"], "json_schema");
                assert_eq!(payload["max_tokens"], 128);
            }
            ProviderKind::Ollama => {
                assert_eq!(payload["stream"], false);
                assert_eq!(payload["format"]["required"], json!(["category"]));
                assert_eq!(payload["options"]["num_predict"], 128);
            }
        }
        task.abort();
    }
}
#[tokio::test]
async fn refusal_incomplete_and_schema_failures_are_distinct() {
    let cases = [
        (
            ProviderKind::OpenAi,
            json!({"status":"completed","output":[{"content":[{"type":"refusal","refusal":"secret text"}]}]}),
            AiErrorKind::Refused,
        ),
        (
            ProviderKind::OpenAi,
            json!({"status":"incomplete","output":[]}),
            AiErrorKind::Incomplete,
        ),
        (
            ProviderKind::Anthropic,
            json!({"stop_reason":"max_tokens","content":[]}),
            AiErrorKind::Incomplete,
        ),
        (
            ProviderKind::Anthropic,
            json!({"stop_reason":"refusal","content":[]}),
            AiErrorKind::Refused,
        ),
        (
            ProviderKind::Ollama,
            json!({"done":false,"message":{"content":"{}"}}),
            AiErrorKind::Incomplete,
        ),
        (
            ProviderKind::Ollama,
            success(ProviderKind::Ollama, "not json private-key"),
            AiErrorKind::InvalidOutput,
        ),
        (
            ProviderKind::OpenAi,
            success(ProviderKind::OpenAi, r#"{"category":"refund"}"#),
            AiErrorKind::InvalidOutput,
        ),
    ];
    for (kind, body, expected) in cases {
        let (url, _, task) = fixture(body, StatusCode::OK, 0).await;
        let client = AiClient::new(
            ProviderConfig {
                kind,
                base_url: url,
                model: "fixture".into(),
            },
            Some(SecretString::new("private-key")),
        )
        .unwrap();
        let e = client.complete(request()).await.unwrap_err();
        assert_eq!(e.kind, expected);
        assert!(!format!("{e:?}").contains("private-key"));
        task.abort();
    }
}
#[tokio::test]
async fn transport_limits_errors_and_no_automatic_retry() {
    for (status, delay, expected) in [
        (StatusCode::UNAUTHORIZED, 0, AiErrorKind::Authentication),
        (StatusCode::TOO_MANY_REQUESTS, 0, AiErrorKind::RateLimited),
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            0,
            AiErrorKind::ProviderUnavailable,
        ),
        (StatusCode::OK, 300, AiErrorKind::Timeout),
    ] {
        let (url, s, task) = fixture(json!({"secret":"private-key"}), status, delay).await;
        let client = AiClient::new(
            ProviderConfig {
                kind: ProviderKind::OpenAi,
                base_url: url,
                model: "fixture".into(),
            },
            Some(SecretString::new("private-key")),
        )
        .unwrap();
        let mut q = request();
        q.timeout_ms = 100;
        let e = client.complete(q).await.unwrap_err();
        assert_eq!(e.kind, expected);
        assert!(!e.safe_message.contains("private-key"));
        assert_eq!(s.captured.lock().unwrap().len(), 1);
        if expected == AiErrorKind::RateLimited {
            assert_eq!(e.retry_after_ms, Some(7000));
        }
        task.abort();
    }
}
#[tokio::test]
async fn oversized_input_response_and_external_schema_references_are_rejected() {
    let (url, s, task) = fixture(json!({"content":"x".repeat(1_048_577)}), StatusCode::OK, 0).await;
    let client = AiClient::new(
        ProviderConfig {
            kind: ProviderKind::Ollama,
            base_url: url,
            model: "fixture".into(),
        },
        None,
    )
    .unwrap();
    let mut q = request();
    q.system = "x".repeat(131_073);
    assert_eq!(
        client.complete(q).await.unwrap_err().kind,
        AiErrorKind::InvalidRequest
    );
    assert!(s.captured.lock().unwrap().is_empty());
    let mut q = request();
    q.output_schema = json!({"$ref":"http://127.0.0.1/private"});
    assert_eq!(
        client.complete(q).await.unwrap_err().kind,
        AiErrorKind::InvalidRequest
    );
    assert!(s.captured.lock().unwrap().is_empty());
    assert_eq!(
        client.complete(request()).await.unwrap_err().kind,
        AiErrorKind::InvalidOutput
    );
    task.abort();
}
#[test]
fn secrets_are_redacted_and_config_rejects_embedded_credentials() {
    assert_eq!(
        format!("{:?}", SecretString::new("private-key")),
        "[REDACTED]"
    );
    let r = AiClient::new(
        ProviderConfig {
            kind: ProviderKind::OpenAi,
            base_url: "https://user:password@example.com/".parse().unwrap(),
            model: "fixture".into(),
        },
        Some(SecretString::new("private-key")),
    );
    assert!(r.is_err());
}

#[tokio::test]
async fn redirect_never_reaches_second_destination() {
    let (destination, captured, other) = fixture(json!({}), StatusCode::OK, 0).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}/", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let target = destination.to_string();
    let router = Router::new().fallback(any(move || {
        let target = target.clone();
        async move { (StatusCode::TEMPORARY_REDIRECT, [("location", target)]) }
    }));
    let redirect = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = AiClient::new(
        ProviderConfig {
            kind: ProviderKind::Anthropic,
            base_url,
            model: "fixture".into(),
        },
        Some(SecretString::new("private-key")),
    )
    .unwrap();
    assert_eq!(
        client.complete(request()).await.unwrap_err().kind,
        AiErrorKind::InvalidRequest
    );
    assert!(captured.captured.lock().unwrap().is_empty());
    redirect.abort();
    other.abort();
}
