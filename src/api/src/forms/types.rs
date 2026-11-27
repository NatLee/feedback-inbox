use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Text,
    Textarea,
    Email,
    Number,
    Select,
    Checkbox,
    Hidden,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    Public,
    #[default]
    Personal,
    Sensitive,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Validation {
    pub max_length: Option<usize>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    #[serde(default)]
    pub integer_only: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldOption {
    pub value: String,
    pub label: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormField {
    pub id: String,
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: FieldKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub help_text: String,
    #[serde(default)]
    pub sensitivity: Sensitivity,
    #[serde(default)]
    pub validation: Validation,
    #[serde(default)]
    pub options: Vec<FieldOption>,
    pub default_value: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormSchema {
    pub schema_version: u16,
    pub title: String,
    pub description: String,
    pub submit_label: String,
    pub success_message: String,
    pub fields: Vec<FormField>,
}
#[derive(Debug, Clone, Serialize)]
pub struct FormVersion {
    pub id: i64,
    pub site_id: i64,
    pub version: i64,
    pub schema: FormSchema,
    pub schema_hash: String,
    pub published_at: String,
    pub retired_at: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct FormDraft {
    pub revision: i64,
    pub schema: FormSchema,
}
pub struct NormalizedAnswers {
    pub values: Value,
    pub submission: crate::intake::Submission,
}
impl FormSchema {
    pub fn legacy(name: &str, required: &[String]) -> Self {
        let fields = crate::intake::FIELDS
            .iter()
            .map(|key| FormField {
                id: crate::auth::random_hex(16),
                key: (*key).into(),
                label: crate::intake::field_label(key).into(),
                kind: match *key {
                    "email" => FieldKind::Email,
                    "message" => FieldKind::Textarea,
                    _ => FieldKind::Text,
                },
                required: required.iter().any(|r| r == key),
                help_text: String::new(),
                sensitivity: if *key == "message" {
                    Sensitivity::Public
                } else {
                    Sensitivity::Personal
                },
                validation: Validation {
                    max_length: Some(match *key {
                        "name" => 100,
                        "email" => 254,
                        "phone" => 50,
                        "company" => 200,
                        _ => 10_000,
                    }),
                    ..Default::default()
                },
                options: vec![],
                default_value: None,
            })
            .collect();
        Self {
            schema_version: 1,
            title: name.into(),
            description: "留下訊息，我們會盡快回覆。".into(),
            submit_label: "送出".into(),
            success_message: "已經收到你的訊息，謝謝。".into(),
            fields,
        }
    }
}
