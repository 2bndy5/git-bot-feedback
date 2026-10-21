//! This submodule implements functionality exclusively specific to GitLab's REST API.

use super::{
    GitlabApiClient, REVIEW_SUMMARY_TAG,
    serde_structs::{MergeRequestInfo, Note},
};
use crate::{
    CommentKind, CommentPolicy, RestApiClient, RestApiRateLimitHeaders, ThreadCommentOptions,
    client::{ClientError, USER_AGENT},
};
use reqwest::{
    Client, Method, Url,
    header::{CONTENT_TYPE, HeaderMap, HeaderValue},
};
use std::{collections::HashMap, env};

impl GitlabApiClient {
    /// Instantiate a [`GitlabApiClient`] object.
    ///
    /// All information is read from [GitLab CI's predefined variables][predefined-vars].
    ///
    /// [predefined-vars]: https://docs.gitlab.com/ci/variables/predefined_variables/
    pub fn new() -> Result<Self, ClientError> {
        let merge_request_iid = match env::var("CI_MERGE_REQUEST_IID") {
            Ok(iid) => Some(iid.parse::<u64>().map_err(|_| {
                ClientError::MalformedEventInfo(format!(
                    "CI_MERGE_REQUEST_IID is not a number: {iid}"
                ))
            })?),
            Err(_) => None,
        };
        let api_url =
            env::var("CI_API_V4_URL").map_err(|e| ClientError::env_var("CI_API_V4_URL", e))?;
        // A trailing slash is needed to preserve the URL's path when joining relative paths.
        let api_url = Url::parse(format!("{}/", api_url.trim_end_matches('/')).as_str())?;

        Ok(Self {
            client: Client::builder()
                .default_headers(Self::make_headers()?)
                .user_agent(USER_AGENT)
                .build()?,
            merge_request_iid,
            event_name: env::var("CI_PIPELINE_SOURCE").unwrap_or(String::from("unknown")),
            api_url,
            project_id: env::var("CI_PROJECT_ID")
                .map_err(|e| ClientError::env_var("CI_PROJECT_ID", e))?,
            sha: env::var("CI_COMMIT_SHA").map_err(|e| ClientError::env_var("CI_COMMIT_SHA", e))?,
            debug_enabled: env::var("CI_DEBUG_TRACE").is_ok_and(|val| &val == "true"),
            rate_limit_headers: RestApiRateLimitHeaders {
                reset: "ratelimit-reset".to_string(),
                remaining: "ratelimit-remaining".to_string(),
                retry: "retry-after".to_string(),
            },
        })
    }

    pub(super) fn make_headers() -> Result<HeaderMap<HeaderValue>, ClientError> {
        let mut headers = HeaderMap::new();
        headers.insert("Accept", HeaderValue::from_str("application/json")?);
        // GitLab ignores a JSON payload if the request does not declare it.
        headers.insert(CONTENT_TYPE, HeaderValue::from_str("application/json")?);
        if let Ok(token) = env::var("GITLAB_TOKEN") {
            log::debug!("Using auth token from GITLAB_TOKEN environment variable");
            let mut val = HeaderValue::from_str(token.as_str())?;
            val.set_sensitive(true);
            headers.insert("PRIVATE-TOKEN", val);
        } else {
            log::warn!(
                "No GITLAB_TOKEN environment variable found! Permission to post comments may be unsatisfied."
            );
        }
        Ok(headers)
    }

    /// Create a URL for the given `path` relative to the CI project's REST API endpoint.
    pub(super) fn project_url(&self, path: &str) -> Result<Url, ClientError> {
        Ok(self
            .api_url
            .join(format!("projects/{}/{path}", self.project_id).as_str())?)
    }

    /// Get information about the merge request with the given `iid`.
    pub(super) async fn get_merge_request(
        &self,
        iid: u64,
    ) -> Result<MergeRequestInfo, ClientError> {
        let url = self.project_url(format!("merge_requests/{iid}").as_str())?;
        let request = self.make_api_request(&self.client, url, Method::GET, None, None)?;
        let response = self
            .send_api_request(&self.client, request, &self.rate_limit_headers)
            .await
            .map_err(|e| e.add_request_context("get merge request info"))?;
        if let Err(e) = response.error_for_status_ref() {
            self.log_response(response, "Failed to get merge request info")
                .await;
            return Err(ClientError::Request(e).add_request_context("get merge request info"));
        }
        serde_json::from_str::<MergeRequestInfo>(&response.text().await?)
            .map_err(|e| ClientError::json("deserialize merge request info", e))
    }

    /// Get the merge request's diff as output by `git diff`.
    ///
    /// Unlike other endpoints that list a merge request's changes, this endpoint does not
    /// omit (collapse) the patch of files with large changes.
    /// This endpoint was added in GitLab v17.9.
    pub(super) async fn get_merge_request_raw_diff(&self, iid: u64) -> Result<String, ClientError> {
        const TASK: &str = "get merge request raw diffs (requires GitLab v17.9 or newer)";
        let url = self.project_url(format!("merge_requests/{iid}/raw_diffs").as_str())?;
        let mut headers = HeaderMap::new();
        headers.insert("Accept", HeaderValue::from_str("text/plain")?);
        let request = self.make_api_request(&self.client, url, Method::GET, None, Some(headers))?;
        let response = self
            .send_api_request(&self.client, request, &self.rate_limit_headers)
            .await
            .map_err(|e| e.add_request_context(TASK))?;
        if let Err(e) = response.error_for_status_ref() {
            self.log_response(response, format!("Failed to {TASK}").as_str())
                .await;
            return Err(ClientError::Request(e).add_request_context(TASK));
        }
        Ok(response.text().await?)
    }

    /// Update existing comment or remove old comment(s) and post a new comment
    pub(super) async fn update_comment(
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

        if !is_lgtm || !options.no_lgtm {
            let payload = HashMap::from([("body", options.mark_comment())]);
            let req_meth = if comment_url.is_some() {
                Method::PUT
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

    /// Remove thread comments previously posted by this software.
    ///
    /// If `delete` is `false`, then the most recent comment is kept and its URL is returned.
    async fn remove_bot_comments(
        &self,
        url: &Url,
        comment_marker: &str,
        delete: bool,
    ) -> Result<Option<Url>, ClientError> {
        let mut comment_url = None;
        // Note, the oldest comments are listed first.
        // This way the last comment found is the most recent one.
        let mut comments_url = Some(Url::parse_with_params(
            url.as_str(),
            &[
                ("sort", "asc"),
                ("order_by", "created_at"),
                ("per_page", "100"),
                ("page", "1"),
            ],
        )?);
        while let Some(endpoint) = comments_url.take() {
            let request = self.make_api_request(&self.client, endpoint, Method::GET, None, None)?;
            let response = self
                .send_api_request(&self.client, request, &self.rate_limit_headers)
                .await
                .map_err(|e| e.add_request_context("get list of existing thread comments"))?;
            if !response.status().is_success() {
                self.log_response(response, "Failed to get list of existing thread comments")
                    .await;
                return Ok(comment_url);
            }
            comments_url = self.try_next_page(response.headers());
            let payload =
                serde_json::from_str::<Vec<Note>>(&response.text().await?).map_err(|e| {
                    ClientError::json("deserialize list of existing thread comments", e)
                })?;
            for comment in payload {
                // Skip notes that are part of a review (and notes generated by GitLab).
                let is_thread_comment = comment.kind.is_none()
                    && !comment.system
                    && comment
                        .body
                        .strip_prefix(comment_marker)
                        .is_some_and(|text| !text.starts_with(REVIEW_SUMMARY_TAG.trim_end()));
                if !is_thread_comment {
                    continue;
                }
                log::debug!(
                    "Found bot comment id {} from user {} ({})",
                    comment.id,
                    comment.author.username,
                    comment.author.id,
                );
                let this_comment_url = Url::parse(format!("{url}/{}", comment.id).as_str())?;
                if delete || comment_url.is_some() {
                    // if not updating: remove all outdated comments
                    // if updating: remove all outdated comments except the last one

                    // use last saved comment_url (if not None) or current comment url
                    let del_url = comment_url.as_ref().unwrap_or(&this_comment_url).clone();
                    let req =
                        self.make_api_request(&self.client, del_url, Method::DELETE, None, None)?;
                    let result = self
                        .send_api_request(&self.client, req, &self.rate_limit_headers)
                        .await
                        .map_err(|e| e.add_request_context("delete old thread comment"))?;
                    self.log_response(result, "Failed to delete old thread comment")
                        .await;
                }
                if !delete {
                    comment_url = Some(this_comment_url)
                }
            }
        }
        Ok(comment_url)
    }
}

/// Sanitize a log group's name into an ID for a collapsible section in GitLab CI logs.
///
/// GitLab only allows letters, numbers, `_`, `.`, and `-` in a section's ID.
pub(super) fn section_id(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || ['_', '.', '-'].contains(&c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::env;

    use super::{GitlabApiClient, section_id};

    #[test]
    fn sanitize_section_id() {
        assert_eq!(section_id("clang-format v18.1.0"), "clang-format_v18.1.0");
        assert_eq!(
            section_id("Analyzing src/main.cpp"),
            "Analyzing_src_main.cpp"
        );
    }

    #[test]
    fn ensure_api_url_has_trailing_slash() {
        unsafe {
            env::set_var(
                "CI_API_V4_URL",
                "https://gitlab.example.com/gitlab/api/v4//",
            );
            env::set_var("CI_PROJECT_ID", "42");
            env::set_var("CI_COMMIT_SHA", "abc123");
            env::remove_var("CI_MERGE_REQUEST_IID");
        }
        let client = GitlabApiClient::new().unwrap();
        assert_eq!(
            client.project_url("merge_requests/7").unwrap().as_str(),
            "https://gitlab.example.com/gitlab/api/v4/projects/42/merge_requests/7"
        );
    }
}
