use thiserror::Error;

pub type QmdResult<T> = Result<T, QmdError>;

#[derive(Debug, Error)]
pub enum QmdError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("unsupported feature: {0}")]
    Unsupported(String),
    #[error("internal error: {0}")]
    Internal(String),
}
