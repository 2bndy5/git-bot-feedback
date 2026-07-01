//! This submodule implements functionality exclusively specific to Github's REST API.

use super::{
    GiteaApiClient,
    serde_structs::{GiteaReviewComment, ReviewInfo, ThreadComment},
};
use crate::{
    CommentKind, CommentPolicy, RestApiClient, RestApiRateLimitHeaders, ReviewComment,
    ThreadCommentOptions,
    client::{ClientError, USER_AGENT, common::PullRequestEventPayload},
};
use reqwest::{
    Client, Method, Url,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use std::{collections::HashMap, env, fs};

impl GiteaApiClient {
    /// Instantiate a [`GiteaApiClient`] object.
    pub fn new() -> Result<Self, ClientError> {
        let event_name = env::var("GITEA_EVENT_NAME").unwrap_or(String::from("unknown"));
        let pull_request = {
            match event_name.as_str() {
                "pull_request" => {
                    // GITEA_*** env vars cannot be overwritten in CI runners on GitHub.
                    let event_payload_path = env::var("GITEA_EVENT_PATH")
                        .map_err(|e| ClientError::env_var("GITEA_EVENT_PATH", e))?;
                    // event payload JSON file can be overwritten/removed in CI runners
                    let file_buf = fs::read_to_string(event_payload_path.clone())
                        .map_err(|e| ClientError::io("read event payload", e))?;
                    let pr_info = serde_json::from_str::<PullRequestEventPayload>(&file_buf)
                        .map_err(|e| ClientError::json("deserialize event payload", e))?
                        .pull_request;
                    Some(pr_info)
                }
                _ => None,
            }
        };
        // GITEA_*** env vars cannot be overwritten in CI runners on GitHub.
        let gh_api_url = format!(
            "{}/api/v1/",
            env::var("GITEA_API_URL").map_err(|e| ClientError::env_var("GITEA_API_URL", e))?
        );
        let api_url = Url::parse(gh_api_url.as_str())?;

        Ok(Self {
            client: Client::builder()
                .default_headers(Self::make_headers()?)
                .user_agent(USER_AGENT)
                .build()?,
            pull_request,
            event_name,
            api_url,
            repo: env::var("GITEA_REPOSITORY")
                .map_err(|e| ClientError::env_var("GITEA_REPOSITORY", e))?,
            sha: env::var("GITEA_SHA").map_err(|e| ClientError::env_var("GITEA_SHA", e))?,
            debug_enabled: env::var("ACTIONS_STEP_DEBUG").is_ok_and(|val| &val == "true"),
            rate_limit_headers: RestApiRateLimitHeaders {
                reset: "x-ratelimit-reset".to_string(),
                remaining: "x-ratelimit-remaining".to_string(),
                retry: "retry-after".to_string(),
            },
        })
    }

    pub(super) fn make_headers() -> Result<HeaderMap<HeaderValue>, ClientError> {
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

    /// Update existing comment or remove old comment(s) and post a new comment
    pub async fn update_comment(
        &self,
        url: Url,
        options: ThreadCommentOptions,
    ) -> Result<(), ClientError> {
        let is_lgtm = options.kind == CommentKind::Lgtm;
        let comment_url = self
            .remove_bot_comments(
                &url,
                &options.marker,
                (options.policy == CommentPolicy::Anew) || (is_lgtm && options.no_lgtm),
            )
            .await?;
        let payload = HashMap::from([("body", options.mark_comment())]);

        if !is_lgtm || !options.no_lgtm {
            // log::debug!("payload body:\n{:?}", payload);
            let req_meth = if comment_url.is_some() {
                Method::PATCH
            } else {
                Method::POST
            };
            let request = self.make_api_request(
                &self.client,
                comment_url.unwrap_or(url),
                req_meth,
                Some(serde_json::json!(&payload).to_string()),
                None,
            )?;
            match self
                .send_api_request(&self.client, request, &self.rate_limit_headers)
                .await
            {
                Ok(response) => {
                    self.log_response(response, "Failed to post thread comment")
                        .await;
                }
                Err(e) => {
                    return Err(e.add_request_context("post thread comment"));
                }
            }
        }
        Ok(())
    }

    /// Remove thread comments previously posted by cpp-linter.
    async fn remove_bot_comments(
        &self,
        url: &Url,
        comment_marker: &str,
        delete: bool,
    ) -> Result<Option<Url>, ClientError> {
        let mut comment_url = None;
        let mut comments_url = Some(Url::parse_with_params(url.as_str(), &[("page", "1")])?);
        let base_comment_url = format!("{}repos/{}/issues/comments", self.api_url, self.repo);
        while let Some(endpoint) = comments_url.take() {
            let request = self.make_api_request(&self.client, endpoint, Method::GET, None, None)?;
            let result = self
                .send_api_request(&self.client, request, &self.rate_limit_headers)
                .await;
            match result {
                Err(e) => {
                    return Err(e.add_request_context("get list of existing thread comments"));
                }
                Ok(response) => {
                    if !response.status().is_success() {
                        self.log_response(
                            response,
                            "Failed to get list of existing thread comments",
                        )
                        .await;
                        return Ok(comment_url);
                    }
                    comments_url = self.try_next_page(response.headers());
                    let payload =
                        serde_json::from_str::<Vec<ThreadComment>>(&response.text().await?);
                    match payload {
                        Err(e) => {
                            return Err(ClientError::json(
                                "deserialize list of existing thread comments",
                                e,
                            ));
                        }
                        Ok(payload) => {
                            for comment in payload {
                                if comment.body.starts_with(comment_marker) {
                                    log::debug!(
                                        "Found bot comment id {} from user {} ({})",
                                        comment.id,
                                        comment.user.login,
                                        comment.user.id,
                                    );
                                    let this_comment_url = Url::parse(
                                        format!("{base_comment_url}/{}", comment.id).as_str(),
                                    )?;
                                    if delete || comment_url.is_some() {
                                        // if not updating: remove all outdated comments
                                        // if updating: remove all outdated comments except the last one

                                        // use last saved comment_url (if not None) or current comment url
                                        let del_url = if let Some(last_url) = &comment_url {
                                            last_url
                                        } else {
                                            &this_comment_url
                                        };
                                        let req = self.make_api_request(
                                            &self.client,
                                            del_url.clone(),
                                            Method::DELETE,
                                            None,
                                            None,
                                        )?;
                                        match self
                                            .send_api_request(
                                                &self.client,
                                                req,
                                                &self.rate_limit_headers,
                                            )
                                            .await
                                        {
                                            Ok(result) => {
                                                if !result.status().is_success() {
                                                    self.log_response(
                                                        result,
                                                        "Failed to delete old thread comment",
                                                    )
                                                    .await;
                                                }
                                            }
                                            Err(e) => {
                                                return Err(e.add_request_context(
                                                    "delete old thread comment",
                                                ));
                                            }
                                        }
                                    }
                                    if !delete {
                                        comment_url = Some(this_comment_url)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(comment_url)
    }

    /// Fetch existing comments for a specific PR review.
    async fn get_review_comments(
        &self,
        review_id: i64,
        pr_number: i64,
        marker: &str,
    ) -> Result<Vec<GiteaReviewComment>, ClientError> {
        let mut review_comments = vec![];
        let mut comments_url = Some(Url::parse(
            format!(
                "{}repos/{}/pulls/{pr_number}/reviews/{review_id}/comments",
                self.api_url, self.repo
            )
            .as_str(),
        )?);
        while let Some(url) = comments_url.take() {
            let request = self.make_api_request(&self.client, url, Method::GET, None, None)?;
            let result = self
                .send_api_request(&self.client, request, &self.rate_limit_headers)
                .await;
            match result {
                Err(e) => {
                    return Err(e.add_request_context("get comments for a review"));
                }
                Ok(response) => {
                    if !response.status().is_success() {
                        self.log_response(response, "Failed to get comments for a review")
                            .await;
                        continue;
                    }
                    comments_url = self.try_next_page(response.headers());
                    let comments_payload =
                        serde_json::from_str::<Vec<GiteaReviewComment>>(&response.text().await?);
                    match comments_payload {
                        Err(e) => {
                            return Err(ClientError::json("deserialize comments for a review", e));
                        }
                        Ok(comments) => {
                            for comment in comments {
                                if comment.body.starts_with(marker) {
                                    review_comments.push(comment);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(review_comments)
    }

    /// Fetch existing reviews for a PR, filtering to only those with body starting with marker.
    pub(super) async fn get_existing_review_comments(
        &self,
        pr_number: i64,
        marker: &str,
    ) -> Result<Vec<ReviewInfo>, ClientError> {
        let mut reviews = Vec::new();
        let mut reviews_url = Some(Url::parse_with_params(
            format!(
                "{}repos/{}/pulls/{pr_number}/reviews",
                self.api_url, self.repo,
            )
            .as_str(),
            &[("page", "1")],
        )?);

        while let Some(endpoint) = reviews_url.take() {
            let request = self.make_api_request(&self.client, endpoint, Method::GET, None, None)?;
            let result = self
                .send_api_request(&self.client, request, &self.rate_limit_headers)
                .await;
            match result {
                Err(e) => {
                    return Err(e.add_request_context("get existing PR reviews"));
                }
                Ok(response) => {
                    if !response.status().is_success() {
                        self.log_response(response, "Failed to get existing PR reviews")
                            .await;
                        break;
                    }
                    reviews_url = self.try_next_page(response.headers());
                    let payload = serde_json::from_str::<Vec<ReviewInfo>>(&response.text().await?);
                    match payload {
                        Err(e) => {
                            return Err(ClientError::json("deserialize existing PR reviews", e));
                        }
                        Ok(payload) => {
                            for mut review in payload {
                                if review.body.starts_with(marker) {
                                    log::debug!(
                                        "Found bot review id {} with {} comments",
                                        review.id,
                                        review.comments_count
                                    );
                                    if review.comments_count > 0 {
                                        review.comments = self
                                            .get_review_comments(review.id, pr_number, marker)
                                            .await?;
                                    }
                                    reviews.push(review);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(reviews)
    }

    /// Check if an existing review comment matches a proposed comment.
    ///
    /// Matching is based on: path, line position, and comment body (with marker stripped).
    pub(super) fn match_review_comment(
        existing: &GiteaReviewComment,
        proposed: &ReviewComment,
        marker: &str,
    ) -> bool {
        let proposed_body = if !proposed.comment.starts_with(marker) {
            format!("{marker}{}", proposed.comment)
        } else {
            proposed.comment.clone()
        };

        // Path must match
        existing.path == proposed.path
        // Check line position: proposed line_end maps to new_position
        // If proposed has line_start, it should map to the line range
        && existing.new_position == proposed.line_end as i64
        // existing comment body must start with the marker
        && existing.body.starts_with(marker)
        // compare comment bodies
        && existing.body == proposed_body
    }

    /// Delete outdated review comments and reviews by their IDs.
    pub(super) async fn delete_outdated_review_comments(
        &self,
        pr_number: i64,
        comment_ids: Vec<i64>,
        review_ids: Vec<i64>,
        delete: bool,
    ) -> Result<(), ClientError> {
        let base_url = format!("{}repos/{}/pulls/{pr_number}", self.api_url, self.repo);
        // resolve individual comments first
        for comment_id in comment_ids {
            let comment_url =
                Url::parse(format!("{base_url}/comments/{comment_id}/resolve").as_str())?;
            let request =
                self.make_api_request(&self.client, comment_url, Method::POST, None, None)?;
            match self
                .send_api_request(&self.client, request, &self.rate_limit_headers)
                .await
            {
                Ok(result) => {
                    self.log_response(result, "Failed to resolve outdated review comment")
                        .await;
                }
                Err(e) => {
                    return Err(e.add_request_context("resolve outdated review comment"));
                }
            }
        }

        // Dismiss reviews (mark as outdated)
        for review_id in review_ids {
            let (request, log_prompt) = if delete {
                let url = Url::parse(format!("{base_url}/reviews/{review_id}").as_str())?;
                let request =
                    self.make_api_request(&self.client, url, Method::DELETE, None, None)?;
                (request, "Failed to delete outdated review")
            } else {
                let url =
                    Url::parse(format!("{base_url}/reviews/{review_id}/dismissals").as_str())?;
                let body = serde_json::json!({
                    "message": "outdated review",
                    "priors": false // do not dismiss all prior reviews, only this one
                });
                let request = self.make_api_request(
                    &self.client,
                    url,
                    Method::POST,
                    Some(body.to_string()),
                    None,
                )?;
                (request, "Failed to dismiss outdated review")
            };

            match self
                .send_api_request(&self.client, request, &self.rate_limit_headers)
                .await
            {
                Ok(result) => {
                    self.log_response(result, log_prompt).await;
                }
                Err(e) => {
                    return Err(e.add_request_context(log_prompt));
                }
            }
        }

        Ok(())
    }
}
