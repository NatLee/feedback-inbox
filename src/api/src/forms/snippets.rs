use super::{FieldKind, FormVersion};
use crate::{
    error::{AppError, AppResult},
    notify::escape_html,
};
use serde_json::{json, Map, Value};
pub fn generate(format: &str, version: &FormVersion, endpoint: &str) -> AppResult<String> {
    let mut sample = Map::new();
    for f in &version.schema.fields {
        sample.insert(
            f.key.clone(),
            match f.kind {
                FieldKind::Email => json!("visitor@example.com"),
                FieldKind::Number => json!(f.validation.min.unwrap_or(1.0)),
                FieldKind::Checkbox => json!(true),
                FieldKind::Select => {
                    json!(f.options.first().map(|o| o.value.as_str()).unwrap_or(""))
                }
                FieldKind::Hidden => json!(f.default_value.as_deref().unwrap_or("context")),
                _ => json!(f.label),
            },
        );
    }
    sample.insert("_form_version".into(), json!(version.version));
    sample.insert("_submission_id".into(), json!("REPLACE_WITH_UNIQUE_ID"));
    sample.insert("_gotcha".into(), json!(""));
    let data = serde_json::to_string_pretty(&sample)?;
    let quoted = serde_json::to_string(endpoint)?;
    Ok(match format {
  "html"=>format!("<form action=\"{}\" method=\"post\">\n{}\n  <input type=\"hidden\" name=\"_form_version\" value=\"{}\">\n  <input name=\"_gotcha\" style=\"display:none\" tabindex=\"-1\" autocomplete=\"off\">\n  <button>{}</button>\n</form>\n<!-- For retries, generate a new _submission_id per new submission on your server. -->",escape_html(endpoint),super::render::controls(&version.schema,&Value::Null),version.version,escape_html(&version.schema.submit_label)),
  "fetch"=>format!("const data = {data};\ndata._submission_id = crypto.randomUUID();\nconst response = await fetch({quoted}, {{\n  method: 'POST', credentials: 'omit',\n  headers: {{ 'Content-Type': 'application/json', 'Accept': 'application/json' }},\n  body: JSON.stringify(data),\n}});\nconst result = await response.json();\nif (!response.ok) throw new Error(result.error || `HTTP ${{response.status}}`);\n// Keep the same _submission_id only when retrying this exact submission."),
  "curl"=>format!("# Replace _submission_id with a fresh UUID for each new message.\ncurl --fail-with-body {} -H 'Content-Type: application/json' -H 'Accept: application/json' --data-binary @- <<'FEEDBACK_JSON'\n{data}\nFEEDBACK_JSON",shell_quote(endpoint)),
  "node"=>format!("// Server-side example. Authenticate/rate-limit your own route and validate its input.\nimport {{ randomUUID }} from 'node:crypto';\nconst data = {data};\ndata._submission_id = randomUUID();\nconst response = await fetch({quoted}, {{\n  method: 'POST', headers: {{ 'Content-Type': 'application/json' }},\n  body: JSON.stringify(data), signal: AbortSignal.timeout(10000),\n}});\nif (!response.ok) throw new Error((await response.json()).error);\n// A server proxy alone does not verify member identity; use a signed identity token."),
  "iframe"=>format!("<iframe src=\"{}/embed\" title=\"{}\" width=\"100%\" height=\"760\" loading=\"lazy\" style=\"border:0\"></iframe>\n<a href=\"{}\" target=\"_blank\" rel=\"noopener\">在新分頁填寫表單</a>",escape_html(endpoint),escape_html(&version.schema.title),escape_html(endpoint)),
  _=>return Err(AppError::bad("不支援的整合格式")),
 })
}
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
