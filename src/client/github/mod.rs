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

#[cfg(feature = "file-changes")]
use crate::{FileDiffLines, FileFilter, LinesChangedOnly, client::send_api_request, parse_diff};
#[cfg(feature = "file-changes")]
use reqwest::Method;
#[cfg(feature = "file-changes")]
use std::{collections::HashMap, path::Path};

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
    /// This prints a line to indicate the beginning of a related group of [`log`] statements.
    ///
    /// For apps' [`log`] implementations, this function's [`log::info`] output needs to have
    /// no prefixed data.
    /// Such behavior can be identified by the log target `"CI_LOG_GROUPING"`.
    ///
    /// ```
    /// # struct MyAppLogger;
    /// impl log::Log for MyAppLogger {
    /// #    fn enabled(&self, metadata: &log::Metadata) -> bool {
    /// #        log::max_level() > metadata.level()
    /// #    }
    ///     fn log(&self, record: &log::Record) {
    ///         if record.target() == "CI_LOG_GROUPING" {
    ///             println!("{}", record.args());
    ///         } else {
    ///             println!(
    ///                 "[{:>5}]{}: {}",
    ///                 record.level().as_str(),
    ///                 record.module_path().unwrap_or_default(),
    ///                 record.args()
    ///             );
    ///         }
    ///     }
    /// #    fn flush(&self) {}
    /// }
    /// ```
    fn start_log_group(name: &str) {
        log::info!(target: "CI_LOG_GROUPING", "::group::{name}");
    }

    /// This prints a line to indicate the ending of a related group of [`log`] statements.
    ///
    /// See also [`GithubApiClient::start_log_group`] about special handling of
    /// the log target `"CI_LOG_GROUPING"`.
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

    #[cfg(feature = "file-changes")]
    #[cfg_attr(docsrs, doc(cfg(feature = "file-changes")))]
    async fn get_list_of_changed_files(
        &self,
        file_filter: &FileFilter,
        lines_changed_only: &LinesChangedOnly,
    ) -> Result<HashMap<String, FileDiffLines>, RestClientError> {
        let is_pr = self.is_pr_event();
        let url_path = if is_pr {
            format!("pulls/{}/files", self.pull_request)
        } else {
            format!("commits/{}", self.sha)
        };
        let url = self
            .api_url
            .join("repos/")?
            .join(format!("{}/", &self.repo).as_str())?
            .join(url_path.as_str())?;
        let mut url = Some(Url::parse_with_params(url.as_str(), &[("page", "1")])?);
        let mut files: HashMap<String, FileDiffLines> = HashMap::new();
        while let Some(ref endpoint) = url {
            let request =
                Self::make_api_request(&self.client, endpoint.as_str(), Method::GET, None, None)?;
            let response =
                send_api_request(&self.client, request, &self.rate_limit_headers).await?;
            url = Self::try_next_page(response.headers());
            let body = response.text().await?;
            let files_list = if !is_pr {
                let json_value: serde_structs::PushEventFiles = serde_json::from_str(&body)?;
                json_value.files
            } else {
                serde_json::from_str::<Vec<serde_structs::GithubChangedFile>>(&body)?
            };
            for file in files_list {
                let ext = Path::new(&file.filename).extension().unwrap_or_default();
                if !file_filter
                    .extensions
                    .contains(&ext.to_string_lossy().to_string())
                {
                    continue;
                }
                if let Some(patch) = file.patch {
                    let diff = format!(
                        "diff --git a/{old} b/{new}\n--- a/{old}\n+++ b/{new}\n{patch}\n",
                        old = file.previous_filename.unwrap_or(file.filename.clone()),
                        new = file.filename,
                    );
                    for (name, info) in parse_diff(&diff, file_filter, lines_changed_only) {
                        files.entry(name).or_insert(info);
                    }
                } else if file.changes == 0 {
                    // file may have been only renamed.
                    // include it in case files-changed-only is enabled.
                    files.entry(file.filename).or_default();
                }
                // else changes are too big (per git server limits) or we don't care
            }
        }
        Ok(files)
    }
}
