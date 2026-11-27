use super::{FieldKind, FormSchema, FormVersion};
use crate::notify::escape_html as e;
use serde_json::Value;

pub const STYLE: &str = r#"
:root{color-scheme:light dark;--bg:#fbf8f3;--paper:#fffdf9;--line:#e2d9cd;--ink:#382e28;--muted:#7f7063;--accent:#b5412b}
@media(prefers-color-scheme:dark){:root{--bg:#191512;--paper:#261f1b;--line:#514338;--ink:#f2ece3;--muted:#b9aa9a;--accent:#f1a78a}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.7 -apple-system,'Noto Sans TC','PingFang TC',sans-serif;padding:24px 16px}main{max-width:600px;margin:auto;padding:28px;border:1px solid var(--line);background:var(--paper);border-radius:12px}header{border-bottom:1px solid var(--line);padding-bottom:16px;margin-bottom:20px}.stamp{color:var(--accent);font-size:11px;letter-spacing:.15em}h1{font-size:24px;font-weight:600;margin:8px 0}p{margin:8px 0}.muted,.hint{font-size:13px;color:var(--muted)}label{display:block;margin:18px 0 6px;font-weight:600;font-size:14px}input,textarea,select{font:inherit;max-width:100%;width:100%;background:transparent;color:inherit;border:1px solid var(--line);padding:10px;border-radius:6px}textarea{min-height:130px}input[type=checkbox]{width:auto}button,.button{display:inline-block;font:inherit;font-weight:600;padding:10px 22px;border:0;border-radius:6px;background:var(--accent);color:var(--paper);margin-top:22px;cursor:pointer;text-decoration:none}input:focus,textarea:focus,select:focus,button:focus-visible{outline:2px solid var(--accent);outline-offset:3px}.trap{position:absolute;left:-9999px}.error{border:1px solid var(--accent);padding:12px;color:var(--accent)}a{color:var(--accent)}footer{margin-top:24px;border-top:1px solid var(--line);padding-top:12px;font-size:12px;color:var(--muted)}@media(max-width:480px){body{padding:12px}main{padding:18px}}
"#;

pub fn controls(schema: &FormSchema, values: &Value) -> String {
    let mut out = String::new();
    for f in &schema.fields {
        let key = e(&f.key);
        let label = e(&f.label);
        let max = f.validation.max_length.unwrap_or(match f.kind {
            FieldKind::Textarea => 2000,
            FieldKind::Email => 254,
            FieldKind::Hidden => 500,
            _ => 1000,
        });
        let attrs = format!(
            "id=\"field-{}\" name=\"{key}\"{}",
            f.id,
            if f.required { " required" } else { "" }
        );
        let value = values
            .get(&f.key)
            .and_then(Value::as_str)
            .or(f.default_value.as_deref())
            .unwrap_or("");
        let control = match f.kind {
            FieldKind::Textarea => format!(
                "<textarea {attrs} maxlength=\"{max}\">{}</textarea>",
                e(value)
            ),
            FieldKind::Select => {
                let mut options = "<option value=\"\">請選擇</option>".to_owned();
                for o in &f.options {
                    options.push_str(&format!(
                        "<option value=\"{}\"{}>{}</option>",
                        e(&o.value),
                        if value == o.value { " selected" } else { "" },
                        e(&o.label)
                    ));
                }
                format!("<select {attrs}>{options}</select>")
            }
            FieldKind::Checkbox => format!(
                "<input {attrs} type=\"checkbox\" value=\"true\"{}>",
                if values
                    .get(&f.key)
                    .is_some_and(|v| v == true || v == "true" || v == "on")
                {
                    " checked"
                } else {
                    ""
                }
            ),
            FieldKind::Number => format!(
                "<input {attrs} type=\"number\" value=\"{}\" step=\"{}\"{}{}>",
                e(value),
                if f.validation.integer_only {
                    "1"
                } else {
                    "any"
                },
                f.validation
                    .min
                    .map(|v| format!(" min=\"{v}\""))
                    .unwrap_or_default(),
                f.validation
                    .max
                    .map(|v| format!(" max=\"{v}\""))
                    .unwrap_or_default()
            ),
            _ => format!(
                "<input {attrs} type=\"{}\" maxlength=\"{max}\" value=\"{}\">",
                match f.kind {
                    FieldKind::Email => "email",
                    FieldKind::Hidden => "hidden",
                    _ if f.key == "phone" => "tel",
                    _ => "text",
                },
                e(value)
            ),
        };
        if f.kind == FieldKind::Hidden {
            out.push_str(&control);
            continue;
        }
        out.push_str(&format!(
            "<label for=\"field-{}\">{label}{}</label>{control}",
            f.id,
            if f.required {
                " <span aria-label=\"必填\">*</span>"
            } else {
                ""
            }
        ));
        if !f.help_text.is_empty() {
            out.push_str(&format!("<p class=\"hint\">{}</p>", e(&f.help_text)));
        }
    }
    out
}
pub fn form(
    version: &FormVersion,
    action: &str,
    submission_id: &str,
    values: &Value,
    error: Option<&str>,
) -> String {
    let s = &version.schema;
    format!("<header><span class=\"stamp\">FEEDBACK / 留一封來信</span><h1>{}</h1><p class=\"muted\">{}</p></header>{}<form method=\"post\" action=\"{}\">{}<input type=\"hidden\" name=\"_form_version\" value=\"{}\"><input type=\"hidden\" name=\"_submission_id\" value=\"{}\"><div class=\"trap\" aria-hidden=\"true\"><label for=\"_gotcha\">這一格請留白</label><input id=\"_gotcha\" name=\"_gotcha\" tabindex=\"-1\" autocomplete=\"off\"></div><button type=\"submit\">{}</button></form><footer>表單版本 {} · 內容將送到網站負責人的收件匣。</footer>",e(&s.title),e(&s.description),error.map(|m|format!("<p role=\"alert\" class=\"error\">{}</p>",e(m))).unwrap_or_default(),e(action),controls(s,values),version.version,e(submission_id),e(&s.submit_label),version.version)
}
pub fn document(title: &str, body: &str) -> String {
    format!("<!doctype html><html lang=\"zh-Hant\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><meta name=\"robots\" content=\"noindex\"><title>{}</title><style>{STYLE}</style></head><body><main>{body}</main></body></html>",e(title))
}
