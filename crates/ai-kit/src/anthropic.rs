use crate::*;
use serde_json::{json, Value};
pub(crate) fn payload(c: &ProviderConfig, q: &CompletionRequest) -> Value {
    json!({"model":c.model,"system":q.system,"messages":[{"role":"user","content":q.input.to_string()}],"max_tokens":q.max_output_tokens,"output_config":{"format":{"type":"json_schema","schema":q.output_schema}}})
}
pub(crate) fn text(v: &Value) -> Result<String, AiError> {
    match v.get("stop_reason").and_then(Value::as_str) {
        Some("refusal") => {
            return Err(AiError::new(
                AiErrorKind::Refused,
                "Provider declined the request",
            ))
        }
        Some("end_turn") => {}
        _ => {
            return Err(AiError::new(
                AiErrorKind::Incomplete,
                "Provider response is incomplete",
            ))
        }
    }
    let parts = v
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(crate::client::invalid_output)?;
    let mut text = String::new();
    for part in parts {
        if part.get("type").and_then(Value::as_str) == Some("text") {
            text.push_str(
                part.get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(crate::client::invalid_output)?,
            );
        }
    }
    if text.is_empty() {
        return Err(crate::client::invalid_output());
    }
    Ok(text)
}
