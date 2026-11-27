use super::types::*;
use crate::{
    error::{AppError, AppResult},
    intake::{looks_like_email, Extra, Submission},
};
use serde_json::{Map, Value};
use std::collections::HashSet;

const ALIASES: &[&str] = &[
    "fullname",
    "full_name",
    "mail",
    "e-mail",
    "tel",
    "telephone",
    "mobile",
    "company_name",
    "organization",
    "organisation",
    "text",
    "content",
    "body",
    "extra",
];
pub fn validate_schema(schema: &FormSchema, previous: Option<&FormSchema>) -> AppResult<()> {
    if schema.schema_version != 1
        || !(1..=30).contains(&schema.fields.len())
        || schema.fields.iter().all(|f| f.kind == FieldKind::Hidden)
    {
        return Err(AppError::bad("表單需要 1–30 個欄位，至少一個可見欄位"));
    }
    for (s, max, required) in [
        (&schema.title, 60, true),
        (&schema.description, 1000, false),
        (&schema.submit_label, 40, true),
        (&schema.success_message, 1000, false),
    ] {
        if s.chars().count() > max || (required && s.trim().is_empty()) {
            return Err(AppError::bad("表單標題或說明長度不符"));
        }
    }
    let mut ids = HashSet::new();
    let mut keys = HashSet::new();
    for f in &schema.fields {
        if f.id.len() != 32
            || !f
                .id
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            || !ids.insert(&f.id)
            || !keys.insert(&f.key)
        {
            return Err(AppError::bad("欄位 ID 或名稱重複／格式不符"));
        }
        if f.key.is_empty()
            || f.key.len() > 40
            || !f.key.as_bytes()[0].is_ascii_lowercase()
            || !f
                .key
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
            || ALIASES.contains(&f.key.as_str())
        {
            return Err(AppError::bad("欄位名稱保留或格式不符"));
        }
        if f.label.trim().is_empty()
            || f.label.chars().count() > 100
            || f.help_text.chars().count() > 300
        {
            return Err(AppError::bad("欄位標籤或說明太長"));
        }
        let builtin = match f.key.as_str() {
            "email" => Some((FieldKind::Email, 254)),
            "message" => Some((FieldKind::Textarea, 10000)),
            "name" => Some((FieldKind::Text, 100)),
            "phone" => Some((FieldKind::Text, 50)),
            "company" => Some((FieldKind::Text, 200)),
            _ => None,
        };
        if let Some((kind, limit)) = builtin {
            if f.kind != kind || f.validation.max_length.is_some_and(|n| n > limit) {
                return Err(AppError::bad("基本欄位的類型或長度不能變更"));
            }
        }
        let limit = match f.kind {
            FieldKind::Textarea => 10000,
            FieldKind::Email => 254,
            FieldKind::Hidden => 500,
            _ => 2000,
        };
        if f.validation.max_length.is_some_and(|n| n == 0 || n > limit)
            || [f.validation.min, f.validation.max]
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1e12)
            || matches!((f.validation.min,f.validation.max),(Some(a),Some(b)) if a>b)
        {
            return Err(AppError::bad("欄位驗證界線不合法"));
        }
        if f.default_value
            .as_ref()
            .is_some_and(|s| f.kind != FieldKind::Hidden || s.chars().count() > 500)
        {
            return Err(AppError::bad("只有隱藏欄位可以有公開預設值"));
        }
        if f.kind == FieldKind::Select {
            let mut values = HashSet::new();
            if !(1..=50).contains(&f.options.len()) {
                return Err(AppError::bad("選項需要 1–50 個"));
            }
            for o in &f.options {
                if o.value.is_empty()
                    || o.value.chars().count() > 80
                    || o.label.trim().is_empty()
                    || o.label.chars().count() > 100
                    || !values.insert(&o.value)
                {
                    return Err(AppError::bad("選項值重複或長度不符"));
                }
            }
        } else if !f.options.is_empty() {
            return Err(AppError::bad("只有選單欄位可以設定選項"));
        }
        if let Some(old) = previous {
            if let Some(p) = old.fields.iter().find(|p| p.id == f.id || p.key == f.key) {
                if p.id != f.id || p.key != f.key || p.kind != f.kind {
                    return Err(AppError::bad(
                        "已發布欄位的 ID、名稱與類型不可變更；請新增欄位",
                    ));
                }
            }
        }
    }
    Ok(())
}
pub fn validate_answers(
    schema: &FormSchema,
    input: &Value,
    legacy: bool,
) -> AppResult<NormalizedAnswers> {
    let object = input
        .as_object()
        .ok_or_else(|| AppError::bad("內容要是一個 JSON 物件"))?;
    let old = if legacy {
        Submission::from_json(input).map_err(AppError::bad)?
    } else {
        Submission::default()
    };
    if legacy {
        let required: Vec<String> = schema
            .fields
            .iter()
            .filter(|f| f.required && crate::intake::FIELDS.contains(&f.key.as_str()))
            .map(|f| f.key.clone())
            .collect();
        old.validate(&required).map_err(AppError::bad)?;
    }
    if !legacy {
        for key in object.keys() {
            if !schema.fields.iter().any(|f| &f.key == key)
                && ![
                    "_gotcha",
                    "_form_version",
                    "_display",
                    "_submission_id",
                    "_identity_token",
                    "_test_token",
                ]
                .contains(&key.as_str())
            {
                return Err(AppError::bad(format!("沒有「{key}」這個欄位")));
            }
        }
    }
    let mut normalized = Map::new();
    let mut projected = Map::new();
    for f in &schema.fields {
        let legacy_value = match f.key.as_str() {
            "name" => Some(&old.name),
            "email" => Some(&old.email),
            "phone" => Some(&old.phone),
            "company" => Some(&old.company),
            "message" => Some(&old.message),
            _ => None,
        };
        let raw = if legacy {
            legacy_value
                .filter(|v| !v.is_empty())
                .map(|v| Value::String(v.clone()))
                .or_else(|| object.get(&f.key).cloned())
        } else {
            object.get(&f.key).cloned()
        };
        let raw = raw
            .or_else(|| f.default_value.clone().map(Value::String))
            .unwrap_or(Value::Null);
        let value = normalize(f, raw, legacy)?;
        normalized.insert(f.id.clone(), value.clone());
        if !value.is_null() {
            projected.insert(f.key.clone(), value);
        }
    }
    let mut submission = Submission::from_json(&Value::Object(projected)).map_err(AppError::bad)?;
    if legacy {
        for extra in old.extra {
            if !schema.fields.iter().any(|f| f.key == extra.name) {
                submission.extra.push(Extra {
                    name: extra.name,
                    value: extra.value,
                });
            }
        }
    }
    if legacy {
        submission.validate(&[]).map_err(AppError::bad)?;
    } else if normalized.values().all(Value::is_null) {
        return Err(AppError::bad("表單是空的"));
    }
    Ok(NormalizedAnswers {
        values: Value::Object(normalized),
        submission,
    })
}
fn normalize(f: &FormField, raw: Value, legacy: bool) -> AppResult<Value> {
    let fail = || AppError::bad(format!("「{}」的內容不符合欄位規則", f.label));
    let empty = raw.is_null() || raw.as_str().is_some_and(|s| s.trim().is_empty());
    if f.kind == FieldKind::Checkbox {
        let checked = match raw {
            Value::Bool(v) => v,
            Value::Null => false,
            Value::String(ref s) if ["on", "true", "1"].contains(&s.as_str()) => true,
            Value::String(ref s) if ["", "false", "0"].contains(&s.as_str()) => false,
            _ => return Err(fail()),
        };
        if f.required && !checked {
            return Err(fail());
        }
        return Ok(Value::Bool(checked));
    }
    if empty {
        if f.required {
            return Err(fail());
        }
        return Ok(Value::Null);
    }
    if f.kind == FieldKind::Number {
        let n = raw
            .as_f64()
            .or_else(|| raw.as_str().and_then(|s| s.parse::<f64>().ok()))
            .ok_or_else(fail)?;
        if !n.is_finite()
            || n.abs() > 1e12
            || f.validation.min.is_some_and(|m| n < m)
            || f.validation.max.is_some_and(|m| n > m)
            || (f.validation.integer_only && n.fract() != 0.0)
        {
            return Err(fail());
        }
        return serde_json::Number::from_f64(n)
            .map(Value::Number)
            .ok_or_else(fail);
    }
    let text = match raw {
        Value::String(s) => s.trim().to_owned(),
        v if legacy => v.to_string(),
        _ => return Err(fail()),
    };
    let max = f.validation.max_length.unwrap_or(match f.kind {
        FieldKind::Textarea => 2000,
        FieldKind::Email => 254,
        FieldKind::Hidden => 500,
        _ => 1000,
    });
    if text.chars().count() > max
        || (f.kind == FieldKind::Email && !looks_like_email(&text))
        || (f.kind == FieldKind::Select && !f.options.iter().any(|o| o.value == text))
    {
        return Err(fail());
    }
    Ok(Value::String(text))
}
