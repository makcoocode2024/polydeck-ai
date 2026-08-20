//! Unified error types for the AI Deck core crate.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("配置错误：{0}")]
    Config(String),

    #[error("凭据错误：{0}")]
    Credential(String),

    #[error("网络错误：{0}")]
    Network(String),

    #[error("协议错误：{0}")]
    Protocol(String),

    #[error("存储错误：{0}")]
    Storage(String),

    #[error("输入无效：{0}")]
    InvalidInput(String),

    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),

    #[error("JSON 错误：{0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP 错误：{0}")]
    Http(#[from] reqwest::Error),

    #[error("数据库错误：{0}")]
    Database(String),

    #[error("加密错误：{0}")]
    Encryption(String),

    #[error("内部错误：{0}")]
    Internal(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl From<AppError> for String {
    fn from(err: AppError) -> Self {
        err.to_string()
    }
}
