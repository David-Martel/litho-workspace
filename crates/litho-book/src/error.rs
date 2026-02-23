use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LithoBookError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("File not found: {path}")]
    FileNotFound { path: String },

    #[error("Invalid file path: {path}")]
    InvalidPath { path: String },

    #[error("Directory scan error: {0}")]
    DirectoryScan(String),

    #[error("Server error: {0}")]
    Server(String),

    #[error("Configuration error: {0}")]
    Config(String),
}

impl From<&LithoBookError> for StatusCode {
    fn from(err: &LithoBookError) -> Self {
        match err {
            LithoBookError::FileNotFound { .. } => StatusCode::NOT_FOUND,
            LithoBookError::InvalidPath { .. } => StatusCode::BAD_REQUEST,
            LithoBookError::Json(_) => StatusCode::INTERNAL_SERVER_ERROR,
            LithoBookError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            LithoBookError::DirectoryScan(_) => StatusCode::INTERNAL_SERVER_ERROR,
            LithoBookError::Server(_) => StatusCode::INTERNAL_SERVER_ERROR,
            LithoBookError::Config(_) => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse for LithoBookError {
    fn into_response(self) -> Response {
        tracing::error!("{}", self);
        let status: StatusCode = StatusCode::from(&self);
        (status, self.to_string()).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_not_found_status() {
        let err = LithoBookError::FileNotFound { path: "x".into() };
        assert_eq!(StatusCode::from(&err), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_invalid_path_status() {
        let err = LithoBookError::InvalidPath { path: "x".into() };
        assert_eq!(StatusCode::from(&err), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_json_error_status() {
        let json_err: serde_json::Error = serde_json::from_str::<String>("invalid").unwrap_err();
        let err = LithoBookError::Json(json_err);
        assert_eq!(StatusCode::from(&err), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_io_error_status() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let err = LithoBookError::Io(io_err);
        assert_eq!(StatusCode::from(&err), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_directory_scan_status() {
        let err = LithoBookError::DirectoryScan("fail".into());
        assert_eq!(StatusCode::from(&err), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_server_error_status() {
        let err = LithoBookError::Server("fail".into());
        assert_eq!(StatusCode::from(&err), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_config_error_status() {
        let err = LithoBookError::Config("bad".into());
        assert_eq!(StatusCode::from(&err), StatusCode::BAD_REQUEST);
    }
}
