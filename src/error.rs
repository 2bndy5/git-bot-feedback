use thiserror::Error;

use crate::OutputVariable;

/// The possible error emitted by the REST client API
#[derive(Debug, Error)]
pub enum RestClientError {
    /// Error related to making HTTP requests
    #[error("{0}")]
    Request(#[from] reqwest::Error),

    /// Errors related to standard I/O.
    #[error("{0}")]
    Io(#[from] std::io::Error),

    /// Error related to exceeding REST API Rate limits
    #[error("Rate Limit exceeded")]
    RateLimit,

    /// Error emitted when cloning a request object fails.
    #[error("Failed to clone request object for auto-reties")]
    RequestCloneError,

    /// Error emitted when creating header value fails.
    #[error("Tried to create a header value from invalid string data")]
    InvalidHeaderValue(#[from] reqwest::header::InvalidHeaderValue),

    /// Error emitted when parsing a URL fails.
    #[error("{0}")]
    UrlParseError(#[from] url::ParseError),

    /// Error emitted when deserializing/serializing request/response JSON data.
    #[error("{0}")]
    JsonError(#[from] serde_json::Error),

    /// Error emitted when failing to read environment variable
    #[error("{0}")]
    EnvVarError(#[from] std::env::VarError),

    /// An error emitted when encountering an invalid [`OutputVariable`].
    #[error("OutputVariable is malformed: {0}")]
    OutputVarError(OutputVariable),
}
