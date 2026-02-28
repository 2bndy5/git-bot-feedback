//! This module holds functionality specific to using Github's REST API.
//!
//! In the root module, we just implement the RestApiClient trait.
//! In other (private) submodules we implement behavior specific to Github's REST API.

use std::{
    env,
    fs::OpenOptions,
    io::{self, Write},
};

use async_trait::async_trait;
use reqwest::{Client, Method, Url};

use crate::{
    FileAnnotation, OutputVariable, ReviewAction, ReviewOptions, ThreadCommentOptions,
    client::{ClientError, RestApiClient, RestApiRateLimitHeaders},
};
mod graphql;
mod serde_structs;
use serde_structs::{FullReview, PullRequestInfo, PullRequestState, ReviewDiffComment};
mod specific_api;

#[cfg(feature = "file-changes")]
use crate::{FileDiffLines, FileFilter, LinesChangedOnly, parse_diff};
#[cfg(feature = "file-changes")]
use std::{collections::HashMap, path::Path};

/// A structure to work with Github REST API.
pub struct GithubApiClient {
    /// The HTTP request client to be used for all REST API calls.
    client: Client,

    /// The CI run's event payload from the webhook that triggered the workflow.
    pull_request: Option<PullRequestInfo>,

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
#[async_trait]
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
    fn start_log_group(&self, name: &str) {
        log::info!(target: "CI_LOG_GROUPING", "::group::{name}");
    }

    /// This prints a line to indicate the ending of a related group of [`log`] statements.
    ///
    /// See also [`GithubApiClient::start_log_group`] about special handling of
    /// the log target `"CI_LOG_GROUPING"`.
    fn end_log_group(&self, _name: &str) {
        log::info!(target: "CI_LOG_GROUPING", "::endgroup::");
    }

    async fn post_thread_comment(&self, options: ThreadCommentOptions) -> Result<(), ClientError> {
        env::var("GITHUB_TOKEN").map_err(|e| ClientError::env_var("GITHUB_TOKEN", e))?;
        let comments_url = match &self.pull_request {
            Some(pr_event) => {
                if pr_event.locked {
                    return Ok(()); // cannot comment on locked PRs
                }
                self.api_url.join(
                    format!("repos/{}/issues/{}/comments", self.repo, pr_event.number).as_str(),
                )?
            }
            None => self
                .api_url
                .join(format!("repos/{}/commits/{}/comments", self.repo, self.sha).as_str())?,
        };
        self.update_comment(comments_url, options).await
    }

    #[inline]
    fn is_pr_event(&self) -> bool {
        self.pull_request.is_some()
    }

    fn append_step_summary(&self, comment: &str) -> Result<(), ClientError> {
        let gh_out = env::var("GITHUB_STEP_SUMMARY")
            .map_err(|e| ClientError::env_var("GITHUB_STEP_SUMMARY", e))?;
        // step summary MD file can be overwritten/removed in CI runners
        match OpenOptions::new().append(true).open(gh_out) {
            Ok(mut gh_out_file) => writeln!(&mut gh_out_file, "\n{comment}\n")
                .map_err(|e| ClientError::io("write to GITHUB_STEP_SUMMARY file", e)),
            Err(e) => Err(ClientError::io("open GITHUB_STEP_SUMMARY file", e)),
        }
    }

    fn write_output_variables(&self, vars: &[OutputVariable]) -> Result<(), ClientError> {
        if vars.is_empty() {
            // Should probably be an error. This check is only here to prevent needlessly
            // fetching the env var GITHUB_OUTPUT value and opening the referenced file.
            return Ok(());
        }
        let gh_out =
            env::var("GITHUB_OUTPUT").map_err(|e| ClientError::env_var("GITHUB_OUTPUT", e))?;
        match OpenOptions::new().append(true).open(gh_out) {
            Ok(mut gh_out_file) => {
                for out_var in vars {
                    out_var.validate()?;
                    writeln!(&mut gh_out_file, "{out_var}\n")
                        .map_err(|e| ClientError::io("write to GITHUB_OUTPUT file", e))?;
                }
                Ok(())
            }
            Err(e) => Err(ClientError::io("open GITHUB_OUTPUT file", e)),
        }
    }

    fn write_file_annotations(&self, annotations: &[FileAnnotation]) -> Result<(), ClientError> {
        if annotations.is_empty() {
            // Should probably be an error.
            // This check is only here to prevent needlessly locking stdout.
            return Ok(());
        }
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        for annotation in annotations {
            writeln!(&mut handle, "{annotation}\n")
                .map_err(|e| ClientError::io("write to file annotation to stdout", e))?;
        }
        handle
            .flush()
            .map_err(|e| ClientError::io("flush stdout with file annotations", e))?;
        Ok(())
    }

    #[cfg(feature = "file-changes")]
    #[cfg_attr(docsrs, doc(cfg(feature = "file-changes")))]
    async fn get_list_of_changed_files(
        &self,
        file_filter: &FileFilter,
        lines_changed_only: &LinesChangedOnly,
        _base_diff: Option<String>,
        _ignore_index: bool,
    ) -> Result<HashMap<String, FileDiffLines>, ClientError> {
        let (url, is_pr) = match &self.pull_request {
            Some(pr_event) => (
                self.api_url.join(
                    format!("repos/{}/pulls/{}/files", self.repo, pr_event.number).as_str(),
                )?,
                true,
            ),
            None => (
                self.api_url
                    .join(format!("repos/{}/commits/{}", self.repo, self.sha).as_str())?,
                false,
            ),
        };
        let mut url = Some(Url::parse_with_params(url.as_str(), &[("page", "1")])?);
        let mut files: HashMap<String, FileDiffLines> = HashMap::new();
        while let Some(ref endpoint) = url {
            let request =
                self.make_api_request(&self.client, endpoint.to_owned(), Method::GET, None, None)?;
            let response = self
                .send_api_request(&self.client, request, &self.rate_limit_headers)
                .await
                .map_err(|e| e.add_request_context("get list of changed files"))?;
            url = self.try_next_page(response.headers());
            let body = response.text().await?;
            let files_list = if !is_pr {
                let json_value: serde_structs::PushEventFiles = serde_json::from_str(&body)
                    .map_err(|e| ClientError::json("deserialize list of changed files", e))?;
                json_value.files
            } else {
                serde_json::from_str::<Vec<serde_structs::GithubChangedFile>>(&body)
                    .map_err(|e| ClientError::json("deserialize list of changed files", e))?
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

    async fn cull_pr_reviews(&mut self, options: &mut ReviewOptions) -> Result<(), ClientError> {
        if let Some(pr_info) = self.pull_request.as_ref() {
            if pr_info.locked
                || (!options.allow_closed && pr_info.state == PullRequestState::Closed)
            {
                return Ok(());
            }
            env::var("GITHUB_TOKEN").map_err(|e| ClientError::env_var("GITHUB_TOKEN", e))?;

            // Check existing comments to see if we can reuse any of them.
            // This also removes duplicate comments (if any) from the `options.comments`.
            let keep_reviews = self.check_reused_comments(options).await?;
            // Next hide/resolve any previous reviews that are completely outdated.
            let url = self
                .api_url
                .join(format!("repos/{}/pulls/{}/reviews", self.repo, pr_info.number).as_str())?;
            self.hide_outdated_reviews(url, keep_reviews, &options.marker)
                .await?;
        }
        Ok(())
    }

    async fn post_pr_review(&mut self, options: &ReviewOptions) -> Result<(), ClientError> {
        if let Some(pr_info) = self.pull_request.as_ref() {
            if (!options.allow_draft && pr_info.draft)
                || (!options.allow_closed && pr_info.state == PullRequestState::Closed)
                || pr_info.locked
            {
                return Ok(());
            }
            env::var("GITHUB_TOKEN").map_err(|e| ClientError::env_var("GITHUB_TOKEN", e))?;
            let url = self
                .api_url
                .join(format!("repos/{}/pulls/{}/reviews", self.repo, pr_info.number).as_str())?;
            let payload = FullReview {
                event: match options.action {
                    ReviewAction::Comment => String::from("COMMENT"),
                    ReviewAction::Approve => String::from("APPROVE"),
                    ReviewAction::RequestChanges => String::from("REQUEST_CHANGES"),
                },
                body: format!("{}{}", options.marker, options.summary),
                comments: options
                    .comments
                    .iter()
                    .map(ReviewDiffComment::from)
                    .map(|mut r| {
                        if !r.body.starts_with(&options.marker) {
                            r.body = format!("{}{}", options.marker, r.body);
                        }
                        r
                    })
                    .collect(),
            };
            let request = self.make_api_request(
                &self.client,
                url,
                Method::POST,
                Some(
                    serde_json::to_string(&payload)
                        .map_err(|e| ClientError::json("serialize PR review payload", e))?,
                ),
                None,
            )?;
            let response = self
                .send_api_request(&self.client, request, &self.rate_limit_headers)
                .await;
            match response {
                Ok(response) => {
                    self.log_response(response, "Failed to post PR review")
                        .await;
                }
                Err(e) => {
                    return Err(e.add_request_context("post PR review"));
                }
            }
        }
        Ok(())
    }
}
