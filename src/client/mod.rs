//! A module to contain traits and structs that are needed by the rest of the git-bot-feedback crate's API.
use std::{env, fmt::Debug, time::Duration};

use async_trait::async_trait;
use chrono::DateTime;
use reqwest::{Client, Method, Request, Response, Url, header::HeaderMap};

use crate::{FileAnnotation, OutputVariable, RestClientError, ReviewOptions, ThreadCommentOptions};

#[cfg(feature = "github")]
mod github;
#[cfg(feature = "github")]
pub use github::GithubApiClient;

#[cfg(not(any(feature = "github", feature = "custom-git-server-impl")))]
compile_error!(
    "At least one Git server implementation (eg. 'github') should be enabled via `features`"
);

#[cfg(feature = "file-changes")]
use crate::{FileDiffLines, FileFilter, LinesChangedOnly, parse_diff};
#[cfg(feature = "file-changes")]
use std::{collections::HashMap, process::Command};

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

/// The [`Result::Err`] type returned for fallible functions in this trait.
pub(crate) type ClientError = RestClientError;

/// The number of attempts made when contending a secondary rate limit in REST API requests.
pub(crate) const MAX_RETRIES: u8 = 5;

/// A custom trait that templates necessary functionality with a Git server's REST API.
#[async_trait]
pub trait RestApiClient {
    /// This prints a line to indicate the beginning of a related group of log statements.
    fn start_log_group(&self, name: &str) {
        log::info!(target: "CI_LOG_GROUPING", "start_log_group: {name}");
    }

    /// This prints a line to indicate the ending of a related group of log statements.
    fn end_log_group(&self, name: &str) {
        log::info!(target: "CI_LOG_GROUPING", "end_log_group: {name}");
    }

    /// Is the current CI event **trigger** a Pull Request?
    ///
    /// This **will not** check if a push event's instigating commit is part of any PR.
    fn is_pr_event(&self) -> bool;

    /// A way to get the list of changed files in the context of the CI event.
    ///
    /// This method will parse diff blobs and return a list of changed files.
    ///
    /// The default implementation uses `git diff` to get the list of changed files.
    /// So, the default implementation requires `git` installed and a non-shallow checkout.
    ///
    /// Other implementations use the Git server's REST API to get the list of changed files.
    #[cfg(feature = "file-changes")]
    #[cfg_attr(docsrs, doc(cfg(feature = "file-changes")))]
    async fn get_list_of_changed_files(
        &self,
        file_filter: &FileFilter,
        lines_changed_only: &LinesChangedOnly,
        base_diff: Option<String>,
        ignore_index: bool,
    ) -> Result<HashMap<String, FileDiffLines>, ClientError> {
        let git_status = if ignore_index {
            0
        } else {
            match Command::new("git").args(["status", "--short"]).output() {
                Err(e) => {
                    return Err(ClientError::io("invoke `git status`", e));
                }
                Ok(output) => {
                    if output.status.success() {
                        String::from_utf8_lossy(&output.stdout)
                            .to_string()
                            // trim last newline to prevent an extra empty line being counted as a changed file
                            .trim_end_matches('\n')
                            .lines()
                            // we only care about staged changes
                            .filter(|l| !l.starts_with(' '))
                            .count()
                    } else {
                        let err_msg = String::from_utf8_lossy(&output.stderr).to_string();
                        return Err(ClientError::GitCommand(err_msg));
                    }
                }
            }
        };
        let mut diff_args = vec!["diff".to_string()];
        if git_status != 0 {
            // There are changes in the working directory.
            // So, compare include the staged changes.
            diff_args.push("--staged".to_string());
        }
        if let Some(base) = base_diff {
            match Command::new("git")
                .args(["rev-parse", base.as_str()])
                .output()
            {
                Err(e) => {
                    return Err(ClientError::Io {
                        task: format!("invoke `git rev-parse {base}` to validate reference"),
                        source: e,
                    });
                }
                Ok(output) => {
                    if output.status.success() {
                        diff_args.push(base);
                    } else if base.chars().all(|c| c.is_ascii_digit()) {
                        // if all chars form a decimal number, then
                        // try using it as a number of parents from HEAD
                        diff_args.push(format!("HEAD~{base}"));
                        // note, if still not a valid git reference, then
                        // the error will be raised by the `git diff` command later
                    } else {
                        let err_msg = String::from_utf8_lossy(&output.stderr).to_string();
                        // Given diff base did not resolve to a valid git reference
                        return Err(ClientError::GitCommand(err_msg));
                    }
                }
            }
        } else if git_status == 0 {
            // No base diff provided and there are no staged changes,
            // just get the diff of the last commit.
            diff_args.push("HEAD~1".to_string());
        }
        match Command::new("git").args(&diff_args).output() {
            Err(e) => Err(ClientError::Io {
                task: format!("invoke `git {}`", diff_args.join(" ")),
                source: e,
            }),
            Ok(output) => {
                if output.status.success() {
                    let diff_str = String::from_utf8_lossy(&output.stdout).to_string();
                    let files = parse_diff(&diff_str, file_filter, lines_changed_only);
                    Ok(files)
                } else {
                    let err_msg = String::from_utf8_lossy(&output.stderr).to_string();
                    Err(ClientError::GitCommand(err_msg))
                }
            }
        }
    }

    /// A way to post feedback to the Git server's GUI.
    ///
    /// The given [`ThreadCommentOptions::comment`] should be compliant with
    /// the Git server's requirements (ie. the comment length is within acceptable limits).
    async fn post_thread_comment(&self, options: ThreadCommentOptions) -> Result<(), ClientError>;

    /// Appends a given comment to the CI workflow's summary page.
    ///
    /// This is the least obtrusive and recommended for push events.
    /// Not all Git servers natively support this type of feedback.
    /// GitHub and Gitea are known to support this.
    /// For all other git servers, this is a non-op returning [`Ok`]
    fn append_step_summary(&self, comment: &str) -> Result<(), ClientError> {
        let _ = comment;
        Ok(())
    }

    /// Resolve outdated PR review comments and remove duplicate/reused comments.
    ///
    /// This should be used before [`Self::post_pr_review()`] to avoid posting duplicates of existing comments.
    /// The [`ReviewOptions::comments`] will be modified to only include comments that should be posted for the current PR review.
    /// After calling this function, the [`ReviewOptions::summary`] can be made to reflect the actual review being posted.
    ///
    /// The [`ReviewOptions::marker`] is used to identify comments from this software.
    /// The [`ReviewOptions::delete_review_comments`] flag will delete outdated review comments.
    /// The [`ReviewOptions::delete_review_comments`] flag does not apply to review summary comments nor
    /// threads of discussion within a review.
    /// A review summary comment will only be hidden/collapsed when all comments in the corresponding
    /// review are resolved.
    ///
    /// This function does nothing for non-PR events.
    async fn cull_pr_reviews(&mut self, options: &mut ReviewOptions) -> Result<(), ClientError>;

    /// Post a PR review based on the given options.
    ///
    /// This is expected to be used after calling [`Self::cull_pr_reviews()`] to
    /// avoid posting duplicates of existing comments. Once the duplicates are filtered out,
    /// the [`ReviewOptions::summary`] can be made to reflect the actual review being posted.
    ///
    /// This function does nothing for non-PR events.
    async fn post_pr_review(&mut self, options: &ReviewOptions) -> Result<(), ClientError>;

    /// Sets the given `vars` as output variables.
    ///
    /// These variables are designed to be consumed by other steps in the CI workflow.
    fn write_output_variables(&self, vars: &[OutputVariable]) -> Result<(), ClientError>;

    /// Sets the given `annotations` as file annotations.
    ///
    /// Not all Git servers support this on their free tiers, namely GitLab.
    fn write_file_annotations(&self, annotations: &[FileAnnotation]) -> Result<(), ClientError> {
        println!("{annotations:#?}");
        Ok(())
    }

    /// Construct a HTTP request to be sent.
    ///
    /// The idea here is that this method is called before [`send_api_request()`].
    /// ```ignore
    /// let request = Self::make_api_request(
    ///     &self.client,
    ///     Url::parse("https://example.com").unwrap(),
    ///     Method::GET,
    ///     None,
    ///     None,
    /// ).unwrap();
    /// let response = send_api_request(&self.client, request, &self.rest_api_headers);
    /// match response.await {
    ///     Ok(res) => todo!(handle response),
    ///     Err(e) => todo!(handle failure),
    /// }
    /// ```
    fn make_api_request(
        &self,
        client: &Client,
        url: Url,
        method: Method,
        data: Option<String>,
        headers: Option<HeaderMap>,
    ) -> Result<Request, ClientError> {
        let mut req = client.request(method, url);
        if let Some(h) = headers {
            req = req.headers(h);
        }
        if let Some(d) = data {
            req = req.body(d);
        }
        req.build()
            .map_err(|e| ClientError::add_request_context(ClientError::Request(e), "build request"))
    }

    /// A convenience function to send HTTP requests and respect a REST API rate limits.
    ///
    /// This method respects both primary and secondary rate limits.
    /// In the event where the secondary rate limits is reached,
    /// this function will wait for a time interval (if specified by the server) and retry afterward.
    async fn send_api_request(
        &self,
        client: &Client,
        request: Request,
        rate_limit_headers: &RestApiRateLimitHeaders,
    ) -> Result<Response, ClientError> {
        for i in 0..MAX_RETRIES {
            let response = client
                .execute(request.try_clone().ok_or(ClientError::CannotCloneRequest)?)
                .await?;
            if [403u16, 429u16].contains(&response.status().as_u16()) {
                // rate limit may have been exceeded

                // check if primary rate limit was violated
                let mut requests_remaining = None;
                if let Some(remaining) = response.headers().get(&rate_limit_headers.remaining) {
                    requests_remaining = Some(remaining.to_str()?.parse::<i64>()?);
                } else {
                    // NOTE: I guess it is sometimes valid for a response to
                    // not include remaining rate limit attempts
                    log::debug!("Response headers do not include remaining API usage count");
                }
                if requests_remaining.is_some_and(|v| v <= 0) {
                    if let Some(reset_value) = response.headers().get(&rate_limit_headers.reset)
                        && let Some(reset) =
                            DateTime::from_timestamp(reset_value.to_str()?.parse::<i64>()?, 0)
                    {
                        return Err(ClientError::RateLimitPrimary(reset));
                    }
                    return Err(ClientError::RateLimitNoReset);
                }

                // check if secondary rate limit is violated. If so, then backoff and try again.
                if let Some(retry_value) = response.headers().get(&rate_limit_headers.retry) {
                    let interval = Duration::from_secs(
                        retry_value.to_str()?.parse::<u64>()? + (i as u64).pow(2),
                    );
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
                    continue;
                }
            }
            return Ok(response);
        }
        Err(ClientError::RateLimitSecondary)
    }

    /// Gets the URL for the next page from the headers in a paginated response.
    ///
    /// Returns [`None`] if current response is the last page.
    fn try_next_page(&self, headers: &HeaderMap) -> Option<Url> {
        if let Some(links) = headers.get("link")
            && let Ok(pg_str) = links.to_str()
        {
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
        None
    }

    async fn log_response(&self, response: Response, context: &str) {
        if let Err(e) = response.error_for_status_ref() {
            log::error!("{}: {e:?}", context.to_owned());
            if let Ok(text) = response.text().await {
                log::error!("{text}");
            }
        }
    }
}
