//! Python-aware types (requests, responses, and exceptions) used by the `GitClient` wrapper.
use std::{collections::HashMap, str::FromStr};

use ::git_bot_feedback::{RestClientError, client::RestApiRateLimitHeaders};
use pyo3::{create_exception, exceptions::PyException, exceptions::PyValueError, prelude::*};
use reqwest::{
    Method, Url,
    header::{HeaderMap, HeaderName, HeaderValue},
};

create_exception!(
    git_bot_feedback,
    GitBotFeedbackError,
    PyException,
    "The base class of all exceptions raised by this package."
);
create_exception!(
    git_bot_feedback,
    RequestError,
    GitBotFeedbackError,
    "Raised when an HTTP request could not be built or sent.\n\nThe ``status`` attribute is the HTTP status code, or ``None`` if there was no response."
);
create_exception!(
    git_bot_feedback,
    RateLimitError,
    GitBotFeedbackError,
    "Raised when a REST API's rate limit is exceeded."
);
create_exception!(
    git_bot_feedback,
    ParseError,
    GitBotFeedbackError,
    "Raised when data (diffs, headers, URLs, JSON, output variables) is malformed."
);
create_exception!(
    git_bot_feedback,
    EnvironmentError,
    GitBotFeedbackError,
    "Raised when the CI environment's event info or variables are missing or malformed."
);

fn request_error(msg: String, status: Option<u16>) -> PyErr {
    let err = RequestError::new_err(msg);
    Python::attach(|py| {
        // Setting an attribute on a fresh exception instance cannot reasonably fail.
        let _ = err.value(py).setattr("status", status); // spell-checker: disable-line
    });
    err
}

/// Convert a native error into the matching Python exception.
pub fn to_py_err(err: RestClientError) -> PyErr {
    let msg = err.to_string();
    match err {
        RestClientError::Request(ref e) | RestClientError::RequestContext { source: ref e, .. } => {
            request_error(msg, e.status().map(|s| s.as_u16()))
        }
        RestClientError::CannotCloneRequest => request_error(msg, None),
        RestClientError::RateLimitNoReset
        | RestClientError::RateLimitPrimary(_)
        | RestClientError::RateLimitSecondary => RateLimitError::new_err(msg),
        RestClientError::DiffError(_)
        | RestClientError::InvalidHeaderValue(_)
        | RestClientError::UnexpectedHeaderValue(_)
        | RestClientError::HeaderParseInt(_)
        | RestClientError::UrlParse(_)
        | RestClientError::Json { .. }
        | RestClientError::OutputVar(_) => ParseError::new_err(msg),
        RestClientError::MalformedEventInfo(_) | RestClientError::EnvVar { .. } => {
            EnvironmentError::new_err(msg)
        }
        _ => GitBotFeedbackError::new_err(msg),
    }
}

/// The rate limit header names used by the given client kind.
pub fn rate_limit_headers(client_kind: &str) -> RestApiRateLimitHeaders {
    let prefix = if client_kind == "gitlab" {
        "ratelimit"
    } else {
        "x-ratelimit"
    };
    RestApiRateLimitHeaders {
        reset: format!("{prefix}-reset"),
        remaining: format!("{prefix}-remaining"),
        retry: "retry-after".to_string(),
    }
}

/// The resolved pieces needed to build a native HTTP request.
pub struct RequestParts {
    pub url: Url,
    pub method: Method,
    pub body: Option<String>,
    pub headers: HeaderMap,
}

/// An HTTP request to send using the client's connection.
///
/// Use :py:attr:`~git_bot_feedback.GitClient.api_url` to assemble the ``url``.
#[pyclass(module = "git_bot_feedback", name = "Request", get_all, set_all)]
#[derive(Clone, Debug)]
pub struct PyRequest {
    /// The full URL to send the request to.
    pub url: String,
    /// The HTTP method (case-insensitive).
    pub method: String,
    /// The request headers. These override the client's default headers.
    pub headers: HashMap<String, String>,
    /// The request body as text.
    pub body: Option<String>,
}

#[pymethods]
impl PyRequest {
    /// Create a request.
    ///
    /// Raises ``ValueError`` when the request is sent if the URL, method, or headers are invalid.
    #[new]
    #[pyo3(
        signature = (url, method=String::from("GET"), headers=None, body=None),
        text_signature = "(url: str, method: str = 'GET', headers: dict[str, str] | None = None, body: str | None = None)"
    )]
    pub fn new(
        url: String,
        method: String,
        headers: Option<HashMap<String, String>>,
        body: Option<String>,
    ) -> Self {
        Self {
            url,
            method,
            headers: headers.unwrap_or_default(),
            body,
        }
    }
}

impl PyRequest {
    /// Validate and resolve this request.
    pub fn resolve(&self) -> PyResult<RequestParts> {
        let url = Url::parse(&self.url)
            .map_err(|e| PyValueError::new_err(format!("Invalid URL '{}': {e}", self.url)))?;

        let method = Method::from_str(&self.method.to_uppercase())
            .map_err(|e| PyValueError::new_err(format!("Invalid method '{}': {e}", self.method)))?;

        let mut headers = HeaderMap::new();
        for (k, v) in &self.headers {
            let name = HeaderName::from_str(k)
                .map_err(|e| PyValueError::new_err(format!("Invalid header name '{k}': {e}")))?;
            let value = HeaderValue::from_str(v).map_err(|e| ParseError::new_err(e.to_string()))?;
            headers.insert(name, value);
        }
        Ok(RequestParts {
            url,
            method,
            body: self.body.clone(),
            headers,
        })
    }
}

/// The response of an HTTP request.
#[pyclass(module = "git_bot_feedback", name = "Response", frozen, get_all)]
#[derive(Clone, Debug)]
pub struct PyResponse {
    /// The HTTP status code.
    pub status: u16,
    /// The response body as text.
    pub body: String,
    /// The response headers (names are lowercase).
    pub headers: HashMap<String, String>,
}

impl PyResponse {
    pub async fn from_reqwest(response: reqwest::Response) -> Result<Self, RestClientError> {
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
            .collect();
        let body = response.text().await?;
        Ok(Self {
            status,
            body,
            headers,
        })
    }
}
