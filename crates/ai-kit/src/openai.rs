use crate::*;
use serde_json::{json, Value};
pub(crate) fn payload(c: &ProviderConfig, q: &CompletionRequest) -> Value {
    json!({"model":c.model,"instructions":q.system,"input":q.input.to_string(),"store":false,"max_output_tokens":q.max_output_tokens,"text":{"format":{"type":"json_schema","name":q.schema_name,"schema":q.output_schema,"strict":true}}})
}
pub(crate) fn text(v: &Value) -> Result<String, AiError> {
    if v.get("status").and_then(Value::as_str) == Some("incomplete") {
        return Err(AiError::new(
            AiErrorKind::Incomplete,
            "Provider response is incomplete",
        ));
    }
    let content = v
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(crate::client::invalid_output)?;
    let mut text = String::new();
    for item in content {
        if let Some(parts) = item.get("content").and_then(Value::as_array) {
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("refusal") => {
                        return Err(AiError::new(
                            AiErrorKind::Refused,
                            "Provider declined the request",
                        ))
                    }
                    Some("output_text") => text.push_str(
                        part.get("text")
                            .and_then(Value::as_str)
                            .ok_or_else(crate::client::invalid_output)?,
                    ),
                    _ => {}
                }
            }
        }
    }
    if v.get("status").and_then(Value::as_str) != Some("completed") || text.is_empty() {
        return Err(crate::client::invalid_output());
    }
    Ok(text)
}
