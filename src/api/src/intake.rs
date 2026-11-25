//! 把表單送來的內容整理成固定的欄位。
//!
//! 固定欄位是 `name`、`email`、`phone`、`company`、`message`；表單上其他的欄位都收進 `extra`，
//! 不用事先設定。JSON 和一般的 HTML 表單（`application/x-www-form-urlencoded`）都收。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 網站可以設成必填的欄位。
pub const FIELDS: [&str; 5] = ["name", "email", "phone", "company", "message"];

/// 這個欄位有填東西的話，當成是機器人送的。表單上要把它藏起來。
pub const HONEYPOT: &str = "_gotcha";

const MAX_EXTRA: usize = 30;
const MAX_EXTRA_NAME: usize = 100;
const MAX_EXTRA_VALUE: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extra {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Submission {
    pub name: String,
    pub email: String,
    pub phone: String,
    pub company: String,
    pub message: String,
    pub extra: Vec<Extra>,
    /// 藏起來的欄位被填了
    pub honeypot: bool,
}

pub fn field_label(field: &str) -> &'static str {
    match field {
        "name" => "姓名",
        "email" => "Email",
        "phone" => "電話",
        "company" => "公司",
        _ => "訊息",
    }
}

fn canonical(key: &str) -> Option<&'static str> {
    Some(match key.trim().to_ascii_lowercase().as_str() {
        "name" | "fullname" | "full_name" => "name",
        "email" | "mail" | "e-mail" => "email",
        "phone" | "tel" | "telephone" | "mobile" => "phone",
        "company" | "company_name" | "organization" | "organisation" => "company",
        "message" | "text" | "content" | "body" => "message",
        _ => return None,
    })
}

/// 把 JSON 的值變成一段文字：陣列用「, 」接起來，物件留著 JSON 的樣子。
fn text_of(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.trim().to_owned(),
        Value::Bool(b) => if *b { "是" } else { "否" }.to_owned(),
        Value::Number(n) => n.to_string(),
        Value::Array(items) => items
            .iter()
            .map(text_of)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(_) => value.to_string(),
    }
}

impl Submission {
    fn set(&mut self, field: &str, value: String) {
        let slot = match field {
            "name" => &mut self.name,
            "email" => &mut self.email,
            "phone" => &mut self.phone,
            "company" => &mut self.company,
            _ => &mut self.message,
        };
        // 同一個欄位出現兩次（例如同時有 text 和 message）時留第一個有內容的
        if slot.is_empty() {
            *slot = value;
        }
    }

    fn push_extra(&mut self, name: &str, value: String) {
        let name = name.trim();
        if name.is_empty() || value.is_empty() {
            return;
        }
        // HTML 表單的複選框會送好幾個同名的欄位：接成一個
        match self.extra.iter_mut().find(|e| e.name == name) {
            Some(existing) => {
                existing.value.push_str(", ");
                existing.value.push_str(&value);
            }
            None => self.extra.push(Extra {
                name: name.to_owned(),
                value,
            }),
        }
    }

    /// `extra` 可以是物件（`{"預算": "10 萬"}`）、`{name, value}` 的陣列，
    /// 或 `{"data": [{name, value}]}`。
    fn push_extras(&mut self, extra: &Value) -> Result<(), String> {
        let named = |items: &[Value], out: &mut Self| -> Result<(), String> {
            for item in items {
                let name = item.get("name").map(text_of).unwrap_or_default();
                if name.is_empty() {
                    return Err("extra 裡每一項都要有 name".into());
                }
                let value = item.get("value").map(text_of).unwrap_or_default();
                out.push_extra(&name, value);
            }
            Ok(())
        };
        match extra {
            Value::Null => Ok(()),
            Value::Array(items) => named(items, self),
            Value::Object(map) => match map.get("data") {
                Some(Value::Array(items)) if map.len() == 1 => named(items, self),
                _ => {
                    for (key, value) in map {
                        self.push_extra(key, text_of(value));
                    }
                    Ok(())
                }
            },
            // 有些表單把 extra 先轉成字串再送
            Value::String(s) if s.trim().is_empty() => Ok(()),
            Value::String(s) => match serde_json::from_str::<Value>(s) {
                Ok(parsed) if parsed.is_object() || parsed.is_array() => self.push_extras(&parsed),
                _ => Err("extra 不是合法的 JSON".into()),
            },
            _ => Err("extra 要是物件或陣列".into()),
        }
    }

    pub fn from_json(body: &Value) -> Result<Self, String> {
        let Value::Object(map) = body else {
            return Err("內容要是一個 JSON 物件".into());
        };
        let mut out = Self::default();
        for (key, value) in map {
            if key == HONEYPOT {
                out.honeypot = !text_of(value).is_empty();
            } else if key == "extra" {
                out.push_extras(value)?;
            } else if key.starts_with('_') {
                // 底線開頭的留給表單自己用
            } else if let Some(field) = canonical(key) {
                out.set(field, text_of(value));
            } else {
                out.push_extra(key, text_of(value));
            }
        }
        Ok(out)
    }

    pub fn from_form(pairs: Vec<(String, String)>) -> Self {
        let mut out = Self::default();
        for (key, value) in pairs {
            let value = value.trim().to_owned();
            if key == HONEYPOT {
                out.honeypot |= !value.is_empty();
            } else if key.starts_with('_') {
            } else if let Some(field) = canonical(&key) {
                out.set(field, value);
            } else {
                // `interest[]` 這種寫法的中括號拿掉
                out.push_extra(key.trim_end_matches("[]"), value);
            }
        }
        out
    }

    /// 檢查必填和長度。錯誤訊息會直接回給送表單的人。
    pub fn validate(&self, required: &[String]) -> Result<(), String> {
        let fields = [
            ("name", &self.name, 100),
            ("email", &self.email, 254),
            ("phone", &self.phone, 50),
            ("company", &self.company, 200),
            ("message", &self.message, 10_000),
        ];
        for (field, value, max) in fields {
            if value.is_empty() && required.iter().any(|r| r == field) {
                return Err(format!("請填寫{}", field_label(field)));
            }
            if value.chars().count() > max {
                return Err(format!("{}太長了（最多 {max} 個字）", field_label(field)));
            }
        }
        if !self.email.is_empty() && !looks_like_email(&self.email) {
            return Err("Email 的格式不對".into());
        }
        if self.extra.len() > MAX_EXTRA {
            return Err(format!("其他欄位太多了（最多 {MAX_EXTRA} 個）"));
        }
        for extra in &self.extra {
            if extra.name.chars().count() > MAX_EXTRA_NAME
                || extra.value.chars().count() > MAX_EXTRA_VALUE
            {
                return Err(format!("「{}」的內容太長了", truncate(&extra.name, 20)));
            }
        }
        let empty = fields.iter().all(|(_, value, _)| value.is_empty()) && self.extra.is_empty();
        if empty {
            return Err("表單是空的".into());
        }
        Ok(())
    }
}

/// 不追求完全符合 RFC，只擋掉明顯打錯的。
pub fn looks_like_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !email.chars().any(|c| c.is_whitespace() || c.is_control())
        && !email.contains(['<', '>', ',', ';'])
}

/// 最多留 `max` 個字，超過的用「…」代替。
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn extra(name: &str, value: &str) -> Extra {
        Extra {
            name: name.into(),
            value: value.into(),
        }
    }

    fn required(fields: &[&str]) -> Vec<String> {
        fields.iter().map(|f| f.to_string()).collect()
    }

    #[test]
    fn json_fields_and_aliases() {
        let s = Submission::from_json(&json!({
            "name": " 王小明 ",
            "email": "ming@example.com",
            "phone": "02-5550-0123",
            "company_name": "範例科技",
            "text": "想了解方案",
            "_subject": "忽略",
        }))
        .unwrap();
        assert_eq!(s.name, "王小明");
        assert_eq!(s.company, "範例科技");
        assert_eq!(s.message, "想了解方案");
        assert!(s.extra.is_empty() && !s.honeypot);
    }

    #[test]
    fn extra_accepts_three_shapes() {
        let object = Submission::from_json(&json!({ "extra": { "預算": "10 萬", "人數": 30 } }));
        assert_eq!(
            object.unwrap().extra,
            [extra("預算", "10 萬"), extra("人數", "30")]
        );
        let list =
            Submission::from_json(&json!({ "extra": [{ "name": "預算", "value": "10 萬" }] }));
        assert_eq!(list.unwrap().extra, [extra("預算", "10 萬")]);
        let wrapped = Submission::from_json(&json!({
            "extra": { "data": [{ "name": "想了解", "value": ["報價", "試用"] }] }
        }));
        assert_eq!(wrapped.unwrap().extra, [extra("想了解", "報價, 試用")]);
        let string = Submission::from_json(&json!({ "extra": "{\"預算\":\"10 萬\"}" }));
        assert_eq!(string.unwrap().extra, [extra("預算", "10 萬")]);
        assert!(Submission::from_json(&json!({ "extra": "not json" })).is_err());
        assert!(Submission::from_json(&json!({ "extra": [{ "value": "x" }] })).is_err());
        assert!(Submission::from_json(&json!(["x"])).is_err());
    }

    #[test]
    fn unknown_keys_become_extras() {
        let s =
            Submission::from_json(&json!({ "message": "hi", "方案": "進階", "訂閱電子報": true }))
                .unwrap();
        assert_eq!(s.extra, [extra("方案", "進階"), extra("訂閱電子報", "是")]);
    }

    #[test]
    fn form_fields() {
        let pair = |k: &str, v: &str| (k.to_owned(), v.to_owned());
        let s = Submission::from_form(vec![
            pair("name", "王小明"),
            pair("message", "你好"),
            pair("interest[]", "報價"),
            pair("interest[]", "試用"),
            pair("_redirect", "https://evil.example"),
            pair("_gotcha", ""),
        ]);
        assert_eq!(s.message, "你好");
        assert_eq!(s.extra, [extra("interest", "報價, 試用")]);
        assert!(!s.honeypot);
        assert!(Submission::from_form(vec![pair("_gotcha", "http://spam")]).honeypot);
    }

    #[test]
    fn validation() {
        let base = Submission {
            name: "王小明".into(),
            email: "ming@example.com".into(),
            message: "你好".into(),
            ..Default::default()
        };
        let all = required(&["name", "email", "message"]);
        assert!(base.validate(&all).is_ok());

        let mut s = base.clone();
        s.message.clear();
        assert_eq!(s.validate(&all).unwrap_err(), "請填寫訊息");
        assert!(s.validate(&required(&["name"])).is_ok());

        let mut s = base.clone();
        s.email = "ming@example".into();
        assert_eq!(s.validate(&all).unwrap_err(), "Email 的格式不對");

        let mut s = base.clone();
        s.message = "字".repeat(10_001);
        assert!(s.validate(&all).is_err());

        let mut s = base.clone();
        s.extra = (0..31).map(|i| extra(&format!("f{i}"), "x")).collect();
        assert!(s.validate(&all).is_err());

        assert_eq!(
            Submission::default().validate(&[]).unwrap_err(),
            "表單是空的"
        );
    }

    #[test]
    fn email_shapes() {
        assert!(looks_like_email("a.b+c@mail.example.com"));
        for bad in [
            "",
            "a",
            "@example.com",
            "a@b",
            "a b@example.com",
            "a@example.com\nBcc: x@example.com",
            "a@b@example.com",
            "a@example.com,b@example.com",
        ] {
            assert!(!looks_like_email(bad), "{bad}");
        }
    }

    #[test]
    fn truncates_by_characters() {
        assert_eq!(truncate("一二三四五", 3), "一二三…");
        assert_eq!(truncate("一二三", 3), "一二三");
    }
}
