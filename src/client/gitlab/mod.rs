//! This module holds functionality specific to using GitLab's REST API.
//!
//! In the root module, we just implement the RestApiClient trait.
//! In other (private) submodules we implement behavior specific to GitLab's REST API.

use std::env;

use async_trait::async_trait;
use chrono::Utc;
use reqwest::{Client, Url};

use super::{ClientError, RestApiClient, RestApiRateLimitHeaders};
use crate::{OutputVariable, ReviewOptions, ThreadCommentOptions};
mod diff_positions;
mod reviews;
mod serde_structs;
mod specific_api;
use specific_api::section_id;

#[cfg(feature = "file-changes")]
use crate::{FileDiffLines, FileFilter, LinesChangedOnly, parse_diff};
#[cfg(feature = "file-changes")]
use reqwest::Method;
#[cfg(feature = "file-changes")]
use specific_api::first_page;
#[cfg(feature = "file-changes")]
use std::{collections::HashMap, path::Path};

/// The ANSI escape sequence that GitLab CI expects around a collapsible section's markers.
const ERASE_LINE: &str = "\x1b[0K";

/// Inserted after the marker of a review's summary comment.
///
/// Unlike other git servers, a review's summary is a regular comment on GitLab.
/// This distinguishes it from the thread comments posted by [`RestApiClient::post_thread_comment()`].
const REVIEW_SUMMARY_TAG: &str = "<!-- review summary -->\n";

/// A structure to work with GitLab REST API.
pub struct GitlabApiClient {
    /// The HTTP request client to be used for all REST API calls.
    client: Client,

    /// The value of the `CI_MERGE_REQUEST_IID` environment variable.
    ///
    /// This is only set in merge request pipelines.
    merge_request_iid: Option<u64>,

    /// The value of the `CI_PIPELINE_SOURCE` environment variable.
    pub event_name: String,

    /// The value of the `CI_API_V4_URL` environment variable.
    api_url: Url,

    /// The value of the `CI_MERGE_REQUEST_PROJECT_ID` environment variable in merge request
    /// pipelines, otherwise the value of the `CI_PROJECT_ID` environment variable.
    project_id: String,

    /// The value of the `CI_COMMIT_SHA` environment variable.
    #[cfg_attr(
        not(feature = "file-changes"),
        allow(dead_code, reason = "only used to get the list of changed files")
    )]
    sha: String,

    /// The value of the `CI_DEBUG_TRACE` environment variable.
    pub debug_enabled: bool,

    /// The response header names that describe the rate limit status.
    rate_limit_headers: RestApiRateLimitHeaders,
}

#[async_trait]
impl RestApiClient for GitlabApiClient {
    fn client_kind(&self) -> String {
        "gitlab".to_string()
    }

    /// This starts a collapsible section in the CI job's log.
    ///
    /// See [GitLab's docs about custom collapsible sections](https://docs.gitlab.com/ci/jobs/job_logs/#custom-collapsible-sections).
    fn start_log_group(&self, name: &str) {
        log::info!(
            target: "CI_LOG_GROUPING",
            "{ERASE_LINE}section_start:{}:{}[collapsed=true]\r{ERASE_LINE}{name}",
            Utc::now().timestamp(),
            section_id(name)
        );
    }

    /// This ends a collapsible section in the CI job's log.
    ///
    /// The given `name` must match the name given to [`Self::start_log_group()`].
    fn end_log_group(&self, name: &str) {
        log::info!(
            target: "CI_LOG_GROUPING",
            "{ERASE_LINE}section_end:{}:{}\r{ERASE_LINE}",
            Utc::now().timestamp(),
            section_id(name)
        );
    }

    fn is_pr_event(&self) -> bool {
        self.merge_request_iid.is_some()
    }

    fn is_debug_enabled(&self) -> bool {
        self.debug_enabled
    }

    fn event_name(&self) -> Option<String> {
        Some(self.event_name.clone())
    }

    fn set_user_agent(&mut self, user_agent: &str) -> Result<(), ClientError> {
        self.client = Client::builder()
            .default_headers(Self::make_headers()?)
            .user_agent(user_agent)
            .build()?;
        Ok(())
    }

    /// Does not support push events, only merge request events.
    async fn post_thread_comment(&self, options: ThreadCommentOptions) -> Result<(), ClientError> {
        let Some(iid) = self.merge_request_iid else {
            log::info!(
                "GitLab support for posting thread comments is limited to merge requests only."
            );
            return Ok(());
        };
        env::var("GITLAB_TOKEN").map_err(|e| ClientError::env_var("GITLAB_TOKEN", e))?;
        if self.get_merge_request(iid).await?.is_locked() {
            return Ok(()); // cannot comment on locked merge requests
        }
        let url = self.merge_request_url(iid, "/notes")?;
        self.update_comment(url, options).await
    }

    async fn cull_pr_reviews(&mut self, options: &mut ReviewOptions) -> Result<(), ClientError> {
        if let Some(iid) = self.merge_request_iid {
            env::var("GITLAB_TOKEN").map_err(|e| ClientError::env_var("GITLAB_TOKEN", e))?;
            let mr_info = self.get_merge_request(iid).await?;
            if mr_info.is_locked() || (!options.allow_closed && mr_info.is_closed()) {
                return Ok(());
            }
            self.check_reused_comments(iid, options).await?;
        }
        Ok(())
    }

    async fn post_pr_review(&mut self, options: &ReviewOptions) -> Result<(), ClientError> {
        if let Some(iid) = self.merge_request_iid {
            env::var("GITLAB_TOKEN").map_err(|e| ClientError::env_var("GITLAB_TOKEN", e))?;
            let mr_info = self.get_merge_request(iid).await?;
            if (!options.allow_draft && mr_info.draft)
                || (!options.allow_closed && mr_info.is_closed())
                || mr_info.is_locked()
            {
                return Ok(());
            }
            let diff_refs = mr_info.diff_refs.ok_or(ClientError::MalformedEventInfo(
                "GitLab has not finished computing the merge request's diff".to_string(),
            ))?;
            self.post_review(iid, &diff_refs, options).await?;
        }
        Ok(())
    }

    /// GitLab CI has no equivalent to output variables, so they are only logged.
    fn write_output_variables(&self, vars: &[OutputVariable]) -> Result<(), ClientError> {
        for var in vars {
            var.validate()?;
            log::info!("{}: {}", var.name, var.value);
        }
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
        let diff = match self.merge_request_iid {
            Some(iid) => self.get_merge_request_raw_diff(iid).await?,
            None => self.get_commit_diff(file_filter).await?,
        };
        parse_diff(&diff, file_filter, lines_changed_only).map_err(ClientError::DiffError)
    }
}

#[cfg(feature = "file-changes")]
impl GitlabApiClient {
    /// Get the diff of the CI pipeline's commit (as a unified diff).
    ///
    /// GitLab does not provide a commit's diff as plain text.
    /// So, this assembles one from the per-file patches.
    async fn get_commit_diff(&self, file_filter: &FileFilter) -> Result<String, ClientError> {
        const TASK: &str = "get list of changed files";
        let url = self.project_url(format!("repository/commits/{}/diff", self.sha).as_str())?;
        let mut next_page = Some(first_page(url));
        let mut diff = String::new();
        while let Some(endpoint) = next_page.take() {
            let request = self.make_api_request(&self.client, endpoint, Method::GET, None, None)?;
            let response = self
                .send_api_request(&self.client, request, &self.rate_limit_headers)
                .await
                .map_err(|e| e.add_request_context(TASK))?;
            if let Err(e) = response.error_for_status_ref() {
                self.log_response(response, format!("Failed to {TASK}").as_str())
                    .await;
                return Err(ClientError::Request(e).add_request_context(TASK));
            }
            next_page = self.try_next_page(response.headers());
            let files =
                serde_json::from_str::<Vec<serde_structs::CommitDiff>>(&response.text().await?)
                    .map_err(|e| ClientError::json("deserialize list of changed files", e))?;
            for file in files {
                if file.deleted_file || !file_filter.is_qualified(Path::new(&file.new_path)) {
                    continue;
                }
                if file.diff.is_empty() && (file.collapsed || file.too_large) {
                    log::warn!(
                        "GitLab omitted the changes to {} because they are too large",
                        file.new_path
                    );
                }
                diff.push_str(
                    format!(
                        "diff --git a/{old} b/{new}\n--- a/{old}\n+++ b/{new}\n{patch}\n",
                        old = file.old_path,
                        new = file.new_path,
                        patch = file.diff.trim_end_matches('\n'),
                    )
                    .as_str(),
                );
            }
        }
        Ok(diff)
    }
}
