use crate::*;
use serde_json::Value;
use std::time::{Duration, Instant};

const MAX_REQUEST: usize = 128 * 1024;
const MAX_RESPONSE: usize = 1024 * 1024;

/// Each client owns a redirect-disabled transport; headers cannot follow a redirect.
pub struct AiClient {
    config: ProviderConfig,
    credential: Option<SecretString>,
    http: reqwest::Client,
}
impl AiClient {
    pub fn new(config: ProviderConfig, credential: Option<SecretString>) -> Result<Self, AiError> {
        if !matches!(config.base_url.scheme(), "http" | "https")
            || config.base_url.host_str().is_none()
            || !config.base_url.username().is_empty()
            || config.base_url.password().is_some()
            || config.base_url.query().is_some()
            || config.base_url.fragment().is_some()
            || config.base_url.path() != "/"
            || config.model.trim().is_empty()
            || config.model.len() > 200
            || config.model.chars().any(char::is_control)
        {
            return Err(AiError::new(
                AiErrorKind::InvalidRequest,
                "Invalid provider configuration",
            ));
        }
        if !matches!(config.kind, ProviderKind::Ollama)
            && credential.as_ref().is_none_or(|s| s.expose().is_empty())
        {
            return Err(AiError::new(
                AiErrorKind::Authentication,
                "Provider credential is required",
            ));
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .user_agent("ai-kit/0.1")
            .build()
            .map_err(|_| {
                AiError::new(AiErrorKind::Network, "Cannot initialize provider transport")
            })?;
        Ok(Self {
            config,
            credential,
            http,
        })
    }
    pub fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            contract_version: 1,
            structured_json: true,
            streaming: false,
            tools: false,
        }
    }
    pub async fn complete(&self, q: CompletionRequest) -> Result<CompletionResult, AiError> {
        if q.contract_version != 1
            || q.max_output_tokens == 0
            || q.max_output_tokens > 32_768
            || !(100..=120_000).contains(&q.timeout_ms)
            || q.schema_name.is_empty()
            || q.schema_name.len() > 64
            || !q
                .schema_name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || q.request_id.len() > 128
            || !safe_schema(&q.output_schema, 0)
        {
            return Err(AiError::new(
                AiErrorKind::InvalidRequest,
                "Invalid completion contract or schema",
            ));
        }
        let validator = jsonschema::validator_for(&q.output_schema)
            .map_err(|_| AiError::new(AiErrorKind::InvalidRequest, "Invalid output JSON schema"))?;
        let (path, body) = match self.config.kind {
            ProviderKind::OpenAi => ("v1/responses", crate::openai::payload(&self.config, &q)),
            ProviderKind::Anthropic => ("v1/messages", crate::anthropic::payload(&self.config, &q)),
            ProviderKind::Ollama => ("api/chat", crate::ollama::payload(&self.config, &q)),
        };
        let bytes = serde_json::to_vec(&body).map_err(|_| {
            AiError::new(
                AiErrorKind::InvalidRequest,
                "Cannot serialize completion request",
            )
        })?;
        if bytes.len() > MAX_REQUEST {
            return Err(AiError::new(
                AiErrorKind::InvalidRequest,
                "Completion request exceeds 128 KiB",
            ));
        }
        let target =
            self.config.base_url.join(path).map_err(|_| {
                AiError::new(AiErrorKind::InvalidRequest, "Invalid provider endpoint")
            })?;
        let mut req = self
            .http
            .post(target)
            .header("content-type", "application/json")
            .timeout(Duration::from_millis(q.timeout_ms))
            .body(bytes);
        if let Some(key) = &self.credential {
            req = match self.config.kind {
                ProviderKind::Anthropic => req.header("x-api-key", key.expose()),
                ProviderKind::OpenAi | ProviderKind::Ollama => req.bearer_auth(key.expose()),
            };
        }
        if self.config.kind == ProviderKind::Anthropic {
            req = req.header("anthropic-version", "2023-06-01");
        }
        let start = Instant::now();
        let mut response = req.send().await.map_err(transport_error)?;
        let status = response.status();
        if !status.is_success() {
            let kind = match status.as_u16() {
                401 | 403 => AiErrorKind::Authentication,
                429 => AiErrorKind::RateLimited,
                500..=599 => AiErrorKind::ProviderUnavailable,
                _ => AiErrorKind::InvalidRequest,
            };
            let mut e = AiError::new(kind, "Provider rejected the request");
            e.retry_after_ms = response
                .headers()
                .get("retry-after")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .map(|n| n.min(300) * 1000);
            return Err(e);
        }
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .filter(|s| s.len() <= 200)
            .map(str::to_owned);
        if response
            .content_length()
            .is_some_and(|n| n > MAX_RESPONSE as u64)
        {
            return Err(oversize());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE {
                return Err(oversize());
            }
            bytes.extend_from_slice(&chunk);
        }
        let raw: Value = serde_json::from_slice(&bytes).map_err(|_| invalid_output())?;
        let text = match self.config.kind {
            ProviderKind::OpenAi => crate::openai::text(&raw)?,
            ProviderKind::Anthropic => crate::anthropic::text(&raw)?,
            ProviderKind::Ollama => crate::ollama::text(&raw)?,
        };
        let output: Value = serde_json::from_str(&text).map_err(|_| invalid_output())?;
        if !validator.is_valid(&output) {
            return Err(invalid_output());
        }
        let usage = match self.config.kind {
            ProviderKind::Ollama => {
                token_usage(raw.get("prompt_eval_count"), raw.get("eval_count"))
            }
            _ => token_usage(
                raw.pointer("/usage/input_tokens"),
                raw.pointer("/usage/output_tokens"),
            ),
        };
        Ok(CompletionResult {
            output,
            resolved_model: raw
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or(&self.config.model)
                .to_owned(),
            provider_request_id: request_id.or_else(|| {
                raw.get("id")
                    .and_then(Value::as_str)
                    .filter(|s| s.len() <= 200)
                    .map(str::to_owned)
            }),
            usage,
            duration_ms: start.elapsed().as_millis().min(u64::MAX as u128) as u64,
        })
    }
}
fn token_usage(input: Option<&Value>, output: Option<&Value>) -> Option<TokenUsage> {
    let input_tokens = input.and_then(Value::as_u64);
    let output_tokens = output.and_then(Value::as_u64);
    (input_tokens.is_some() || output_tokens.is_some()).then_some(TokenUsage {
        input_tokens,
        output_tokens,
    })
}
fn transport_error(e: reqwest::Error) -> AiError {
    AiError::new(
        if e.is_timeout() {
            AiErrorKind::Timeout
        } else if e.is_builder() {
            AiErrorKind::InvalidRequest
        } else {
            AiErrorKind::Network
        },
        "Provider connection failed",
    )
}
fn oversize() -> AiError {
    AiError::new(
        AiErrorKind::InvalidOutput,
        "Provider response exceeds 1 MiB",
    )
}
pub(crate) fn invalid_output() -> AiError {
    AiError::new(
        AiErrorKind::InvalidOutput,
        "Provider output did not match the required schema",
    )
}
fn safe_schema(value: &Value, depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    match value {
        Value::Object(m) => m.iter().all(|(k, v)| {
            (k != "$ref" || v.as_str().is_some_and(|s| s.starts_with('#')))
                && safe_schema(v, depth + 1)
        }),
        Value::Array(a) => a.iter().all(|v| safe_schema(v, depth + 1)),
        _ => true,
    }
}
