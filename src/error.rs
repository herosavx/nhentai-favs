use thiserror::Error;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("HTTP error: {0}")]
    Http(#[from] wreq::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    #[error("Invalid header value: {0}")]
    InvalidHeader(#[from] wreq::header::InvalidHeaderValue),

    #[error("Login failed: {0}")]
    LoginFailed(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Gallery error: {0}")]
    Gallery(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Task join error: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),
}
