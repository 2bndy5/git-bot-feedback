//! A module to contain traits and structs that are needed by the rest of the git-bot-feedback crate's API.
use crate::{OutputVariable, RestClientError, ThreadCommentOptions};
use chrono::DateTime;
use reqwest::{
    Client, IntoUrl, Method, Request, Response, Url,
    header::{HeaderMap, HeaderValue},
};
use std::future::Future;
use std::time::Duration;
use std::{env, fmt::Debug};
mod github;
pub use github::GithubApiClient;

/// The User-Agent header value included in all HTTP requests.
pub static USER_AGENT: &str = concat!(env!("CARGO_CRATE_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// A structure to contain the different forms of headers that
/// describe a REST API's rate limit status.
#[derive(Debug, Clone)]
pub struct RestApiRateLimitHeaders {
    /// The header key of the rate limit's reset time.
    pub reset: String,
    /// The header key of the rate limit's remaining attempts.
    pub remaining: String,
    /// The header key of the rate limit's "backoff" time interval.
    pub retry: String,
}

/// A custom trait that templates necessary functionality with a Git server's REST API.
pub trait RestApiClient {
    /// This prints a line to indicate the beginning of a related group of log statements.
    fn start_log_group(name: &str);

    /// This prints a line to indicate the ending of a related group of log statements.
    fn end_log_group();

    /// A convenience method to create the headers attached to all REST API calls.
    ///
    /// If an authentication token is provided (via environment variable),
    /// this method shall include the relative information.
    fn make_headers() -> Result<HeaderMap<HeaderValue>, RestClientError>;

    /// Is the current CI event **trigger** a Pull Request?
    ///
    /// This **will not** check if a push event's instigating commit is part of any PR.
    fn is_pr_event(&self) -> bool;

    /// A way to post feedback to the Git server's GUI.
    ///
    /// The given [`ThreadCommentOptions::comment`] should be compliant with
    /// the Git server's requirements (ie. the comment length is within acceptable limits).
    fn post_thread_comment(
        &self,
        options: ThreadCommentOptions,
    ) -> impl Future<Output = Result<(), RestClientError>>;

    /// Appends a given comment to the CI workflow's summary page.
    ///
    /// This is the least obtrusive and recommended for push events.
    /// Not all Git servers natively support this type of feedback.
    /// GitHub, and Gitea are known to support this.
    /// For all other git servers, this is a non-op returning [`Ok`]
    fn append_step_summary(comment: &str) -> Result<(), RestClientError> {
        let _ = comment;
        Ok(())
    }

    /// Sets the given `vars` as output variables.
    ///
    /// These variables are designed to be consumed by other steps in the CI workflow.
    fn write_output_variables(vars: &[OutputVariable]) -> Result<(), RestClientError>;

    /// Construct a HTTP request to be sent.
    ///
    /// The idea here is that this method is called before [`send_api_request()`].
    /// ```ignore
    /// let request = Self::make_api_request(
    ///     &self.client,
    ///     "https://example.com",
    ///     Method::GET,
    ///     None,
    ///     None,
    /// ).unwrap();
    /// let response = send_api_request(&self.client, request, &self.rest_api_headers);
    /// match response.await {
    ///     Ok(res) => {/* handle response */}
    ///     Err(e) => {/* handle failure */}
    /// }
    /// ```
    fn make_api_request(
        client: &Client,
        url: impl IntoUrl,
        method: Method,
        data: Option<String>,
        headers: Option<HeaderMap>,
    ) -> Result<Request, RestClientError> {
        let mut req = client.request(method, url);
        if let Some(h) = headers {
            req = req.headers(h);
        }
        if let Some(d) = data {
            req = req.body(d);
        }
        req.build().map_err(RestClientError::Request)
    }

    /// Gets the URL for the next page in a paginated response.
    ///
    /// Returns [`None`] if current response is the last page.
    fn try_next_page(headers: &HeaderMap) -> Option<Url> {
        if let Some(links) = headers.get("link") {
            if let Ok(pg_str) = links.to_str() {
                let pages = pg_str.split(", ");
                for page in pages {
                    if page.ends_with("; rel=\"next\"") {
                        if let Some(link) = page.split_once(">;") {
                            let url = link.0.trim_start_matches("<").to_string();
                            if let Ok(next) = Url::parse(&url) {
                                return Some(next);
                            } else {
                                log::debug!("Failed to parse next page link from response header");
                            }
                        } else {
                            log::debug!("Response header link for pagination is malformed");
                        }
                    }
                }
            }
        }
        None
    }

    fn log_response(response: Response, context: &str) -> impl Future<Output = ()> + Send {
        async move {
            if let Err(e) = response.error_for_status_ref() {
                log::error!("{}: {e:?}", context.to_owned());
                if let Ok(text) = response.text().await {
                    log::error!("{text}");
                }
            }
        }
    }
}

const MAX_RETRIES: u8 = 5;

/// A convenience function to send HTTP requests and respect a REST API rate limits.
///
/// This method respects both primary and secondary rate limits.
/// In the event where  the secondary rate limits is reached,
/// this function will wait for a time interval specified the server and retry afterward.
pub async fn send_api_request(
    client: &Client,
    request: Request,
    rate_limit_headers: &RestApiRateLimitHeaders,
) -> Result<Response, RestClientError> {
    for i in 0..MAX_RETRIES {
        let result = client
            .execute(
                request
                    .try_clone()
                    .ok_or(RestClientError::RequestCloneError)?,
            )
            .await
            .map_err(RestClientError::Request);
        match result {
            Ok(response) => {
                if [403u16, 429u16].contains(&response.status().as_u16()) {
                    // rate limit may have been exceeded

                    // check if primary rate limit was violated
                    let mut requests_remaining = None;
                    if let Some(remaining) = response.headers().get(&rate_limit_headers.remaining) {
                        if let Ok(count) = remaining.to_str() {
                            if let Ok(value) = count.parse::<i64>() {
                                requests_remaining = Some(value);
                            } else {
                                log::debug!(
                                    "Failed to parse i64 from remaining attempts about rate limit: {count}"
                                );
                            }
                        }
                    } else {
                        // NOTE: I guess it is sometimes valid for a response to
                        // not include remaining rate limit attempts
                        log::debug!("Response headers do not include remaining API usage count");
                    }
                    if requests_remaining.is_some_and(|v| v <= 0) {
                        if let Some(reset_value) = response.headers().get(&rate_limit_headers.reset)
                        {
                            if let Ok(epoch) = reset_value.to_str() {
                                if let Ok(value) = epoch.parse::<i64>() {
                                    if let Some(reset) = DateTime::from_timestamp(value, 0) {
                                        log::error!(
                                            "REST API rate limit exceeded! Resets at {reset}"
                                        );
                                        return Err(RestClientError::RateLimit);
                                    }
                                } else {
                                    log::debug!(
                                        "Failed to parse i64 from reset time about rate limit: {epoch}"
                                    );
                                }
                            }
                        } else {
                            log::debug!("Response headers does not include a reset timestamp");
                        }
                        return Err(RestClientError::RateLimit);
                    }

                    // check if secondary rate limit is violated. If so, then backoff and try again.
                    if let Some(retry_value) = response.headers().get(&rate_limit_headers.retry) {
                        if let Ok(retry_str) = retry_value.to_str() {
                            if let Ok(retry) = retry_str.parse::<u64>() {
                                let interval = Duration::from_secs(retry + (i as u64).pow(2));
                                #[cfg(feature = "test-skip-wait-for-rate-limit")]
                                {
                                    // Output a log statement to use the `interval` variable.
                                    log::warn!(
                                        "Skipped waiting {} seconds to expedite test",
                                        interval.as_secs()
                                    );
                                }
                                #[cfg(not(feature = "test-skip-wait-for-rate-limit"))]
                                {
                                    tokio::time::sleep(interval).await;
                                }
                            } else {
                                log::debug!(
                                    "Failed to parse u64 from retry interval about rate limit: {retry_str}"
                                );
                            }
                        }
                        continue;
                    }
                }
                return Ok(response);
            }
            Err(e) => return Err(e),
        }
    }
    log::error!("REST API secondary rate limit exceeded after {MAX_RETRIES} retries.");
    Err(RestClientError::RateLimit)
}
