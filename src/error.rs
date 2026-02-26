//! Error types used across the git-bot-feedback crate.
#[cfg(feature = "file-changes")]
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::client::MAX_RETRIES;

/// The possible errors emitted when validating an [`OutputVariable`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OutputVariableError {
    /// The output variable's name is empty.
    #[error("The output variable's name is empty")]
    NameIsEmpty,
    /// The output variable's name starts with a number.
    #[error("The output variable's name starts with a number: '{0}'")]
    NameStartsWithNumber(String),
    /// The output variable's name contains non-printable characters.
    #[error("The output variable's name contains non-printable characters: '{0}'")]
    NameContainsNonPrintableCharacters(String),
    /// The output variable's value contains non-printable characters.
    #[error("The output variable's value contains non-printable characters: '{0}'")]
    ValueContainsNonPrintableCharacters(String),
}

/// The possible error emitted by the REST client API
#[derive(Debug, Error)]
pub enum RestClientError {
    /// Error related to making HTTP requests
    #[error(transparent)]
    Request(#[from] reqwest::Error),

    /// Error related to making HTTP requests, with additional context about the request that caused the error.
    #[error("Failed to {task}: {source}")]
    RequestContext {
        task: String,
        #[source]
        source: reqwest::Error,
    },

    /// Errors related to standard I/O.
    #[error("Failed to {task}: {source}")]
    Io {
        task: String,
        #[source]
        source: std::io::Error,
    },

    /// Error related to `git` command execution.
    #[error("Git command error: {0}")]
    #[cfg(feature = "file-changes")]
    #[cfg_attr(docsrs, doc(cfg(feature = "file-changes")))]
    GitCommand(String),

    /// Error related to exceeding REST API Rate limits and
    /// no reset time is provided in the response headers.
    #[error("Primary Rate Limit exceeded (no reset time provided)")]
    RateLimitNoReset,

    /// Error related to exceeding REST API Rate limits with a known reset time.
    #[error("Primary Rate Limit exceeded; resets at {0}")]
    RateLimitPrimary(DateTime<Utc>),

    /// Error related to exhausting all retries after hitting REST API Rate limits.
    #[error("Rate Limit exceeded after all {MAX_RETRIES} retries exhausted")]
    RateLimitSecondary,

    /// Error emitted when cloning a request object fails.
    #[error("Failed to clone request object for auto-retries")]
    CannotCloneRequest,

    /// Error emitted when creating header value fails.
    #[error("Tried to create a header value from invalid string data")]
    InvalidHeaderValue(#[from] reqwest::header::InvalidHeaderValue),

    /// Error emitted when converting a header value to string fails.
    #[error("Failed to convert header value to string")]
    UnexpectedHeaderValue(#[from] reqwest::header::ToStrError),

    /// Error emitted when parsing an integer from a header value (string) fails.
    #[error("Failed to parse integer from header value: {0}")]
    HeaderParseInt(#[from] std::num::ParseIntError),

    /// Error emitted when parsing a URL fails.
    #[error("Failed to parse URL:{0}")]
    UrlParse(#[from] url::ParseError),

    /// Error emitted when deserializing/serializing request/response JSON data.
    #[error("Failed to {task}: {source}")]
    Json {
        task: String,
        #[source]
        source: serde_json::Error,
    },

    /// Error emitted when failing to read environment variable
    #[error("Failed to get env var '{name}': {source}")]
    EnvVar {
        name: String,
        #[source]
        source: std::env::VarError,
    },

    /// An error emitted when encountering an invalid [`OutputVariable`](crate::output_variable::OutputVariable).
    #[error("OutputVariable is malformed: {0}")]
    OutputVar(#[from] OutputVariableError),
}

impl RestClientError {
    /// Helper function to create an [`Self::EnvVar`] error with variable name and source error.
    pub fn env_var(name: &str, source: std::env::VarError) -> Self {
        Self::EnvVar {
            name: name.to_string(),
            source,
        }
    }

    /// Helper function to create an [`Self::Io`] error with task context.
    pub fn io(task: &str, source: std::io::Error) -> Self {
        Self::Io {
            task: task.to_string(),
            source,
        }
    }

    /// Builder function to add context to [`Self::Request`] errors.
    ///
    /// Returns a [`Self::RequestContext`] error if `self` is a [`Self::Request`] error.
    /// Otherwise, returns `self` unchanged.
    pub fn add_request_context(self, task: &str) -> Self {
        match self {
            Self::Request(e) => Self::RequestContext {
                task: task.to_string(),
                source: e,
            },
            _ => self,
        }
    }

    /// Helper function to create a [`Self::Json`] error with task context.
    pub fn json(task: &str, source: serde_json::Error) -> Self {
        Self::Json {
            task: task.to_string(),
            source,
        }
    }
}

/// The possible errors emitted by file utilities
#[cfg(feature = "file-changes")]
#[derive(Debug, Error)]
#[cfg_attr(docsrs, doc(cfg(feature = "file-changes")))]
pub enum DirWalkError {
    #[error("Failed to read {path}: {source}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(transparent)]
    OsError(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::RestClientError;

    #[test]
    fn no_added_req_ctx() {
        let err = RestClientError::CannotCloneRequest;
        assert!(matches!(
            err.add_request_context("some task"),
            RestClientError::CannotCloneRequest
        ));
    }
}
