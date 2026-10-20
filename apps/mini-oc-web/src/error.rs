//! 统一错误信封 `{"error":{"code","message"}}`。
//!
//! V2 变更：删除 V1 的 invalid_pcname / device_auth_failed / serve_down
//! （设备端独立 TUI 服务已不存在），新增 auth_mismatch（隧道通但凭据不一致）。

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("invalid target: {0}")]
    InvalidTarget(String),
    /// 设备隧道可达但凭据不一致（用户已重置密钥，设备仍用旧 key）。
    #[error("auth mismatch: {0}")]
    AuthMismatch(String),
    #[error("device offline: {0}")]
    DeviceOffline(String),
    /// 设备返回的 pairing 链接未通过防开放重定向校验。
    #[error("pair url invalid: {0}")]
    PairUrlInvalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("rate limited")]
    RateLimited,
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Copy)]
pub enum ErrorCode {
    Unauthorized,
    InvalidTarget,
    AuthMismatch,
    DeviceOffline,
    PairUrlInvalid,
    NotFound,
    Forbidden,
    RateLimited,
    ServiceUnavailable,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Unauthorized => "unauthorized",
            ErrorCode::InvalidTarget => "invalid_target",
            ErrorCode::AuthMismatch => "auth_mismatch",
            ErrorCode::DeviceOffline => "device_offline",
            ErrorCode::PairUrlInvalid => "pair_url_invalid",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Forbidden => "forbidden",
            ErrorCode::RateLimited => "rate_limited",
            ErrorCode::ServiceUnavailable => "service_unavailable",
            ErrorCode::Internal => "internal",
        }
    }
}

impl AppError {
    pub fn code(&self) -> ErrorCode {
        match self {
            AppError::Unauthorized(_) => ErrorCode::Unauthorized,
            AppError::InvalidTarget(_) => ErrorCode::InvalidTarget,
            AppError::AuthMismatch(_) => ErrorCode::AuthMismatch,
            AppError::DeviceOffline(_) => ErrorCode::DeviceOffline,
            AppError::PairUrlInvalid(_) => ErrorCode::PairUrlInvalid,
            AppError::NotFound(_) => ErrorCode::NotFound,
            AppError::Forbidden(_) => ErrorCode::Forbidden,
            AppError::RateLimited => ErrorCode::RateLimited,
            AppError::ServiceUnavailable(_) => ErrorCode::ServiceUnavailable,
            AppError::Internal(_) => ErrorCode::Internal,
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            AppError::Unauthorized(_) => 401,
            AppError::InvalidTarget(_) => 400,
            AppError::AuthMismatch(_) => 502,
            AppError::DeviceOffline(_) => 502,
            AppError::PairUrlInvalid(_) => 502,
            AppError::NotFound(_) => 404,
            AppError::Forbidden(_) => 403,
            AppError::RateLimited => 429,
            AppError::ServiceUnavailable(_) => 503,
            AppError::Internal(_) => 500,
        }
    }
}

#[derive(Serialize)]
pub struct ErrorBody<'a> {
    pub error: ErrorDetail<'a>,
}

#[derive(Serialize)]
pub struct ErrorDetail<'a> {
    pub code: &'a str,
    pub message: String,
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ErrorBody {
            error: ErrorDetail {
                code: self.code().as_str(),
                message: self.to_string(),
            },
        }
        .serialize(serializer)
    }
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let status = axum::http::StatusCode::from_u16(self.status()).unwrap();
        let mut resp = (status, axum::Json(self)).into_response();
        resp.headers_mut()
            .insert("content-type", "application/json".parse().unwrap());
        resp
    }
}

pub type AppResult<T> = Result<T, AppError>;
