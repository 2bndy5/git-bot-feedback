use std::{env, fs::OpenOptions, io::Write};

use async_trait::async_trait;
use reqwest::{Client, Method};
use url::Url;

use super::{ClientError, RestApiClient, RestApiRateLimitHeaders, common::PullRequestInfo};
use crate::{
    OutputVariable, ReviewAction, ReviewOptions, ThreadCommentOptions,
    client::common::PullRequestState,
};
mod serde_structs;
use serde_structs::{FullReview, ReviewDiffComment};
mod specific_api;

#[cfg(feature = "file-changes")]
use crate::{FileDiffLines, FileFilter, LinesChangedOnly, parse_diff};
#[cfg(feature = "file-changes")]
use reqwest::header::{HeaderMap, HeaderValue};
#[cfg(feature = "file-changes")]
use std::collections::HashMap;

/// A structure to work with Gitea REST API.
pub struct GiteaApiClient {
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

#[async_trait]
impl RestApiClient for GiteaApiClient {
    fn start_log_group(&self, name: &str) {
        log::info!(target: "CI_LOG_GROUPING", "::group::{name}");
    }

    fn end_log_group(&self, _name: &str) {
        log::info!(target: "CI_LOG_GROUPING", "::endgroup::");
    }

    fn is_pr_event(&self) -> bool {
        self.pull_request.is_some()
    }

    fn set_user_agent(&mut self, user_agent: &str) -> Result<(), ClientError> {
        self.client = Client::builder()
            .default_headers(Self::make_headers()?)
            .user_agent(user_agent)
            .build()?;
        Ok(())
    }

    /// Does not support push events, only PR events.
    async fn post_thread_comment(&self, options: ThreadCommentOptions) -> Result<(), ClientError> {
        let comments_url = match &self.pull_request {
            Some(pr_info) => {
                if pr_info.locked {
                    return Ok(()); // cannot comment on locked PRs
                }
                self.api_url.join(
                    format!("repos/{}/issues/{}/comments", self.repo, pr_info.number).as_str(),
                )?
            }
            None => {
                // This feature is supported in non-PR events on other git servers.
                // But Gitea only supports comments on PRs or issues.
                // Leave a informative log entry to highlight this and return early.
                log::info!(
                    "Gitea support for posting thread comments is limited to pull requests only."
                );
                return Ok(());
            }
        };
        self.update_comment(comments_url, options).await
    }

    async fn cull_pr_reviews(&mut self, options: &mut ReviewOptions) -> Result<(), ClientError> {
        if let Some(pr_info) = self.pull_request.as_ref() {
            // Guard checks for unsuitable PR states
            if (!options.allow_draft && pr_info.draft)
                || (!options.allow_closed && pr_info.state == PullRequestState::Closed)
                || pr_info.locked
            {
                return Ok(());
            }

            // Fetch existing reviews from this bot
            let existing_reviews = self
                .get_existing_review_comments(pr_info.number as i64, &options.marker)
                .await?;

            if existing_reviews.is_empty() {
                return Ok(());
            }

            let mut outdated_comment_ids = Vec::new();
            let mut outdated_review_ids = Vec::new();
            let mut reused_comments = std::collections::HashSet::new();

            // Check each existing review for reused comments
            for review in &existing_reviews {
                let mut keep_review = false;

                for existing_comment in &review.comments {
                    let mut keep_comment = false;

                    // Try to match against proposed comments
                    for proposed_comment in options.comments.iter() {
                        if Self::match_review_comment(
                            existing_comment,
                            proposed_comment,
                            &options.marker,
                        ) {
                            log::info!(
                                "Using existing review comment: path='{}', line_end={}",
                                existing_comment.path,
                                existing_comment.new_position
                            );
                            reused_comments.insert(proposed_comment.clone());
                            keep_comment = true;
                            keep_review = true;
                            break;
                        }
                    }

                    // If comment doesn't match any proposed comment, mark for deletion
                    if !keep_comment {
                        outdated_comment_ids.push(existing_comment.id);
                    }
                }

                // If no comments in this review were kept, mark review for deletion
                if !keep_review {
                    outdated_review_ids.push(review.id);
                }
            }

            // Remove reused comments from proposed comments
            options.comments.retain(|c| !reused_comments.contains(c));

            // Delete outdated comments and reviews
            if !outdated_comment_ids.is_empty() || !outdated_review_ids.is_empty() {
                self.delete_outdated_review_comments(
                    pr_info.number as i64,
                    outdated_comment_ids,
                    outdated_review_ids,
                    options.delete_review_comments,
                )
                .await?;
            }
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
            env::var("GITEA_TOKEN").map_err(|e| ClientError::env_var("GITEA_TOKEN", e))?;
            let url = self
                .api_url
                .join(format!("repos/{}/pulls/{}/reviews", self.repo, pr_info.number).as_str())?;
            let payload = FullReview {
                event: match options.action {
                    ReviewAction::Comment => String::from("COMMENT"),
                    ReviewAction::Approve => String::from("APPROVED"),
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
                commit_id: self.sha.clone(),
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

    fn write_output_variables(&self, vars: &[OutputVariable]) -> Result<(), ClientError> {
        if vars.is_empty() {
            // Should probably be an error. This check is only here to prevent needlessly
            // fetching the env var GITEA_OUTPUT value and opening the referenced file.
            return Ok(());
        }
        if let Ok(gh_out) = env::var("GITEA_OUTPUT") {
            return match OpenOptions::new().append(true).open(gh_out) {
                Ok(mut gh_out_file) => {
                    for out_var in vars {
                        out_var.validate()?;
                        writeln!(&mut gh_out_file, "{}={}\n", out_var.name, out_var.value)
                            .map_err(|e| ClientError::io("write to GITEA_OUTPUT file", e))?;
                    }
                    Ok(())
                }
                Err(e) => Err(ClientError::io("write to GITEA_OUTPUT file", e)),
            };
        }
        Ok(())
    }

    fn append_step_summary(&self, comment: &str) -> Result<(), ClientError> {
        if let Ok(gh_out) = env::var("GITEA_STEP_SUMMARY") {
            // step summary MD file can be overwritten/removed in CI runners
            return match OpenOptions::new().append(true).open(gh_out) {
                Ok(mut gh_out_file) => {
                    let result = writeln!(&mut gh_out_file, "\n{comment}\n");
                    result.map_err(|e| ClientError::io("write to GITHUB_STEP_SUMMARY file", e))
                }
                Err(e) => Err(ClientError::io("write to GITHUB_STEP_SUMMARY file", e)),
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
        _base_diff: Option<String>,
        _ignore_index: bool,
    ) -> Result<HashMap<String, FileDiffLines>, ClientError> {
        let url_path = match &self.pull_request {
            Some(pr_info) => format!("repos/{}/pulls/{}.diff", self.repo, pr_info.number),
            None => format!("repos/{}/commits/{}.diff", self.repo, self.sha),
        };
        let url = self.api_url.join(&url_path)?;
        let mut headers = HeaderMap::new();
        headers.insert("Accept", HeaderValue::from_str("text/plain")?);
        let request = self.make_api_request(&self.client, url, Method::GET, None, Some(headers))?;
        let response = self
            .send_api_request(&self.client, request, &self.rate_limit_headers)
            .await?;
        if let Err(e) = response.error_for_status_ref() {
            let body = response.text().await?;
            log::error!("Failed to get list of changed files: {e:?}\n{body}");
            return Err(ClientError::Request(e));
        }
        let body = (response.text()).await?.to_string();
        parse_diff(&body, file_filter, lines_changed_only).map_err(ClientError::DiffError)
    }

    fn client_kind(&self) -> String {
        "gitea".to_string()
    }
}
