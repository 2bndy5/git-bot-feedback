//! This module holds functionality specific to using Github's REST API.
//!
//! In the root module, we just implement the RestApiClient trait.
//! In other (private) submodules we implement behavior specific to Github's REST API.

use crate::{
    OutputVariable, ThreadCommentOptions,
    client::{RestApiClient, RestApiRateLimitHeaders},
    error::RestClientError,
};
use reqwest::{
    Client, Url,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use std::{env, fs::OpenOptions, io::Write};
mod serde_structs;
mod specific_api;

/// A structure to work with Github REST API.
pub struct GithubApiClient {
    /// The HTTP request client to be used for all REST API calls.
    client: Client,

    /// The CI run's event payload from the webhook that triggered the workflow.
    pull_request: i64,

    /// The name of the event that was triggered when running cpp_linter.
    pub event_name: String,

    /// The value of the `GITHUB_API_URL` environment variable.
    api_url: Url,

    /// The value of the `GITHUB_REPOSITORY` environment variable.
    repo: String,

    /// The value of the `GITHUB_SHA` environment variable.
    sha: String,

    /// The value of the `ACTIONS_STEP_DEBUG` environment variable.
    pub debug_enabled: bool,

    /// The response header names that describe the rate limit status.
    rate_limit_headers: RestApiRateLimitHeaders,
}

// implement the RestApiClient trait for the GithubApiClient
impl RestApiClient for GithubApiClient {
    /// This prints a line to indicate the beginning of a related group of log statements.
    fn start_log_group(name: &str) {
        log::info!(target: "CI_LOG_GROUPING", "::group::{name}");
    }

    /// This prints a line to indicate the ending of a related group of log statements.
    fn end_log_group() {
        log::info!(target: "CI_LOG_GROUPING", "::endgroup::");
    }

    fn make_headers() -> Result<HeaderMap<HeaderValue>, RestClientError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Accept",
            HeaderValue::from_str("application/vnd.github.raw+json")?,
        );
        if let Ok(token) = env::var("GITHUB_TOKEN") {
            log::debug!("Using auth token from GITHUB_TOKEN environment variable");
            let mut val = HeaderValue::from_str(format!("token {token}").as_str())?;
            val.set_sensitive(true);
            headers.insert(AUTHORIZATION, val);
        } else {
            log::warn!(
                "No GITHUB_TOKEN environment variable found! Permission to post comments may be unsatisfied."
            );
        }
        Ok(headers)
    }

    async fn post_thread_comment(
        &self,
        options: ThreadCommentOptions,
    ) -> Result<(), RestClientError> {
        let is_pr = self.is_pr_event();
        let comments_url = self
            .api_url
            .join("repos/")?
            .join(format!("{}/", self.repo).as_str())?
            .join(if is_pr { "issues/" } else { "commits/" })?
            .join(
                format!(
                    "{}/",
                    if is_pr {
                        self.pull_request.to_string()
                    } else {
                        self.sha.clone()
                    }
                )
                .as_str(),
            )?
            .join("comments")?;

        self.update_comment(comments_url, options).await
    }

    #[inline]
    fn is_pr_event(&self) -> bool {
        self.pull_request > 0
    }

    fn append_step_summary(comment: &str) -> Result<(), RestClientError> {
        if let Ok(gh_out) = env::var("GITHUB_STEP_SUMMARY") {
            // step summary MD file can be overwritten/removed in CI runners
            return match OpenOptions::new().append(true).open(gh_out) {
                Ok(mut gh_out_file) => {
                    let result = writeln!(&mut gh_out_file, "\n{comment}\n");
                    if let Err(e) = &result {
                        log::error!("Could not write to GITHUB_STEP_SUMMARY file: {e}");
                    }
                    result.map_err(RestClientError::Io)
                }
                Err(e) => {
                    log::error!("GITHUB_STEP_SUMMARY file could not be opened: {e}");
                    Err(RestClientError::Io(e))
                }
            };
        }
        Ok(())
    }

    fn write_output_variables(vars: &[OutputVariable]) -> Result<(), RestClientError> {
        if vars.is_empty() {
            // Should probably be an error. This check is only here to prevent needlessly
            // fetching the env var GITHUB_OUTPUT value and opening the referenced file.
            return Ok(());
        }
        if let Ok(gh_out) = env::var("GITHUB_OUTPUT") {
            return match OpenOptions::new().append(true).open(gh_out) {
                Ok(mut gh_out_file) => {
                    for out_var in vars {
                        if !out_var.validate() {
                            return Err(RestClientError::OutputVarError(out_var.clone()));
                        }
                        if let Err(e) =
                            writeln!(&mut gh_out_file, "{}={}\n", out_var.name, out_var.value)
                        {
                            log::error!("Could not write to GITHUB_OUTPUT file: {e}");
                            return Err(RestClientError::Io(e));
                        }
                    }
                    Ok(())
                }
                Err(e) => {
                    log::error!("GITHUB_OUTPUT file could not be opened: {e}");
                    Err(RestClientError::Io(e))
                }
            };
        }
        Ok(())
    }
}
