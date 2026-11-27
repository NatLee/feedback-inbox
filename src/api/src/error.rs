use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// API 錯誤。回應格式固定是 `{"error": "訊息"}`。
#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    /// 沒登入，或 session 過期。
    Unauthorized(String),
    /// 登入了，但沒有權限；或寫入請求不是同源送來的。
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    /// 登入失敗太多次。
    TooManyRequests(String),
    Internal(anyhow::Error),
    Coded {
        status: u16,
        code: String,
        message: String,
    },
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn coded(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Coded {
            status,
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn bad(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::Forbidden(msg.into())
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }
}

impl AppError {
    pub fn public_parts(self) -> (StatusCode, String, Option<String>) {
        let (status, message, code) = match self {
            Self::Coded {
                status,
                code,
                message,
            } => (
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                message,
                Some(code),
            ),
            Self::BadRequest(m) => (StatusCode::BAD_REQUEST, m, None),
            Self::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m, None),
            Self::Forbidden(m) => (StatusCode::FORBIDDEN, m, None),
            Self::NotFound(m) => (StatusCode::NOT_FOUND, m, None),
            Self::Conflict(m) => (StatusCode::CONFLICT, m, None),
            Self::TooManyRequests(m) => (StatusCode::TOO_MANY_REQUESTS, m, None),
            Self::Internal(err) => {
                tracing::error!("internal error: {err:#}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "伺服器內部錯誤，請稍後重試".into(),
                    None,
                )
            }
        };
        (status, message, code)
    }
}
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message, code) = self.public_parts();
        let mut body = json!({"error":message});
        if let Some(code) = code {
            body["code"] = json!(code);
        }
        (status, Json(body)).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        AppError::Internal(err.into())
    }
}
