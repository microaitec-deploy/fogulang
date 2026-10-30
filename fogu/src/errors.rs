use thiserror::Error;

#[derive(Debug, Error)]
pub enum FoguError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Sled: {0}")]
    Sled(#[from] sled::Error),
    #[error("{0}")]
    Protocol(&'static str),
    #[error("{0}")]
    App(String),
}