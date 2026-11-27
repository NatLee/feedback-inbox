use crate::*;
use serde_json::{json, Value};
pub(crate) fn payload(c: &ProviderConfig, q: &CompletionRequest) -> Value {
    json!({"model":c.model,"messages":[{"role":"system","content":q.system},{"role":"user","content":q.input.to_string()}],"stream":false,"format":q.output_schema,"options":{"num_predict":q.max_output_tokens,"temperature":0}})
}
pub(crate) fn text(v: &Value) -> Result<String, AiError> {
    if v.get("done").and_then(Value::as_bool) != Some(true)
        || v.get("done_reason")
            .and_then(Value::as_str)
            .is_some_and(|r| r != "stop")
    {
        return Err(AiError::new(
            AiErrorKind::Incomplete,
            "Provider response is incomplete",
        ));
    }
    v.pointer("/message/content")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(crate::client::invalid_output)
}
