use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[serde(rename = "openai")]
    OpenAi,
    Anthropic,
    Ollama,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: Url,
    pub model: String,
}
#[derive(Clone)]
pub struct SecretString(String);
impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub contract_version: u16,
    pub system: String,
    pub input: Value,
    pub output_schema: Value,
    pub schema_name: String,
    pub max_output_tokens: u32,
    pub timeout_ms: u64,
    pub request_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResult {
    pub output: Value,
    pub resolved_model: String,
    pub provider_request_id: Option<String>,
    pub usage: Option<TokenUsage>,
    pub duration_ms: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiErrorKind {
    Authentication,
    InvalidRequest,
    UnsupportedCapability,
    RateLimited,
    Timeout,
    Network,
    ProviderUnavailable,
    Refused,
    Incomplete,
    InvalidOutput,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiError {
    pub kind: AiErrorKind,
    pub retry_after_ms: Option<u64>,
    pub safe_message: String,
}
impl AiError {
    pub fn new(kind: AiErrorKind, message: &str) -> Self {
        Self {
            kind,
            retry_after_ms: None,
            safe_message: message.into(),
        }
    }
}
impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.safe_message)
    }
}
impl std::error::Error for AiError {}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub contract_version: u16,
    pub structured_json: bool,
    pub streaming: bool,
    pub tools: bool,
}
