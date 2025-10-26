use std::{env, fs::OpenOptions, io::Write};

use reqwest::{
    Client,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use url::Url;

use super::{RestApiClient, RestApiRateLimitHeaders};
use crate::{OutputVariable, RestClientError, ThreadCommentOptions};
mod serde_structs;
mod specific_api;

#[cfg(feature = "file-changes")]
use crate::{FileDiffLines, FileFilter, LinesChangedOnly, client::send_api_request, parse_diff};
#[cfg(feature = "file-changes")]
use reqwest::Method;
#[cfg(feature = "file-changes")]
use std::collections::HashMap;

/// A structure to work with Gitea REST API.
pub struct GiteaApiClient {
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

impl RestApiClient for GiteaApiClient {
    fn start_log_group(name: &str) {
        log::info!(target: "CI_LOG_GROUPING", "::group::{name}");
    }

    fn end_log_group() {
        log::info!(target: "CI_LOG_GROUPING", "::endgroup::");
    }

    fn make_headers() -> Result<HeaderMap<HeaderValue>, RestClientError> {
        let mut headers = HeaderMap::new();
        headers.insert("Accept", HeaderValue::from_str("application/json")?);
        if let Ok(token) = env::var("GITEA_TOKEN") {
            log::debug!("Using auth token from GITEA_TOKEN environment variable");
            let mut val = HeaderValue::from_str(format!("token {token}").as_str())?;
            val.set_sensitive(true);
            headers.insert(AUTHORIZATION, val);
        } else {
            log::warn!(
                "No GITEA_TOKEN environment variable found! Permission to post comments may be unsatisfied."
            );
        }
        Ok(headers)
    }

    fn is_pr_event(&self) -> bool {
        self.pull_request > 0
    }

    /// Does not support push events, only PR events.
    async fn post_thread_comment(
        &self,
        options: ThreadCommentOptions,
    ) -> Result<(), RestClientError> {
        if !self.is_pr_event() {
            // This feature is supported in non-PR events on other git servers.
            // But Gitea only supports comments on PRs or issues.
            // Leave a informative log entry to highlight this and return early.
            log::info!("Gitea support for posting thread comments is limited to pull requests only.");
            return Ok(());
        }
        let comments_url = self
            .api_url
            .join(format!("repos/{}/issues/{}/comments", self.repo, self.pull_request).as_str())?;

        self.update_comment(comments_url, options).await
    }

    fn write_output_variables(vars: &[OutputVariable]) -> Result<(), RestClientError> {
        if vars.is_empty() {
            // Should probably be an error. This check is only here to prevent needlessly
            // fetching the env var GITEA_OUTPUT value and opening the referenced file.
            return Ok(());
        }
        if let Ok(gh_out) = env::var("GITEA_OUTPUT") {
            return match OpenOptions::new().append(true).open(gh_out) {
                Ok(mut gh_out_file) => {
                    for out_var in vars {
                        if !out_var.validate() {
                            return Err(RestClientError::OutputVarError(out_var.clone()));
                        }
                        if let Err(e) =
                            writeln!(&mut gh_out_file, "{}={}\n", out_var.name, out_var.value)
                        {
                            log::error!("Could not write to GITEA_OUTPUT file: {e}");
                            return Err(RestClientError::Io(e));
                        }
                    }
                    Ok(())
                }
                Err(e) => {
                    log::error!("GITEA_OUTPUT file could not be opened: {e}");
                    Err(RestClientError::Io(e))
                }
            };
        }
        Ok(())
    }

    fn append_step_summary(comment: &str) -> Result<(), RestClientError> {
        if let Ok(gh_out) = env::var("GITEA_STEP_SUMMARY") {
            // step summary MD file can be overwritten/removed in CI runners
            return match OpenOptions::new().append(true).open(gh_out) {
                Ok(mut gh_out_file) => {
                    let result = writeln!(&mut gh_out_file, "\n{comment}\n");
                    if let Err(e) = &result {
                        log::error!("Could not write to GITEA_STEP_SUMMARY file: {e}");
                    }
                    result.map_err(RestClientError::Io)
                }
                Err(e) => {
                    log::error!("GITEA_STEP_SUMMARY file could not be opened: {e}");
                    Err(RestClientError::Io(e))
                }
            };
        }
        Ok(())
    }

    #[cfg(feature = "file-changes")]
    #[cfg_attr(docsrs, doc(cfg(feature = "file-changes")))]
    async fn get_list_of_changed_files(
        &self,
        file_filter: &FileFilter,
        lines_changed_only: &LinesChangedOnly,
    ) -> Result<HashMap<String, FileDiffLines>, RestClientError> {
        let is_pr = self.is_pr_event();
        let url_path = if is_pr {
            format!("repos/{}/pulls/{}.diff", self.repo, self.pull_request)
        } else {
            format!("repos/{}/commits/{}.diff", self.repo, self.sha)
        };
        let url = self.api_url.join(&url_path)?;
        let mut headers = HeaderMap::new();
        headers.insert("Accept", HeaderValue::from_str("text/plain")?);
        let request =
            Self::make_api_request(&self.client, url.as_str(), Method::GET, None, Some(headers))?;
        let response = send_api_request(&self.client, request, &self.rate_limit_headers).await?;
        if let Err(e) = response.error_for_status_ref() {
            let body = response.text().await?;
            log::error!("Failed to get list of changed files: {e:?}\n{body}");
            return Err(RestClientError::Request(e));
        }
        let body = (response.text()).await?.to_string();
        parse_diff(&body, file_filter, lines_changed_only)
    }
}
