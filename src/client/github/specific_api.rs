//! This submodule implements functionality exclusively specific to Github's REST API.

use super::{GithubApiClient, serde_structs::ThreadComment};
use crate::{
    CommentKind, CommentPolicy, RestApiClient, RestApiRateLimitHeaders, ThreadCommentOptions,
    client::{ClientError, USER_AGENT, common::PullRequestEventPayload},
};
use reqwest::{
    Client, Method, Url,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use std::{collections::HashMap, env, fs};

impl GithubApiClient {
    /// Instantiate a [`GithubApiClient`] object.
    pub fn new() -> Result<Self, ClientError> {
        let event_name = env::var("GITHUB_EVENT_NAME").unwrap_or(String::from("unknown"));
        let pull_request = {
            match event_name.as_str() {
                "pull_request" => {
                    // GITHUB_*** env vars cannot be overwritten in CI runners on GitHub.
                    let event_payload_path = env::var("GITHUB_EVENT_PATH")
                        .map_err(|e| ClientError::env_var("GITHUB_EVENT_PATH", e))?;
                    // event payload JSON file can be overwritten/removed in CI runners
                    let file_buf = fs::read_to_string(event_payload_path.clone()).map_err(|e| {
                        ClientError::io(
                            format!("read event payload from {event_payload_path}").as_str(),
                            e,
                        )
                    })?;
                    Some(
                        serde_json::from_str::<PullRequestEventPayload>(&file_buf)
                            .map_err(|e| ClientError::json("deserialize Event Payload", e))?
                            .pull_request,
                    )
                }
                _ => None,
            }
        };
        // GITHUB_*** env vars cannot be overwritten in CI runners on GitHub.
        let gh_api_url = env::var("GITHUB_API_URL").unwrap_or("https://api.github.com".to_string());
        let api_url = Url::parse(format!("{}/", gh_api_url.trim_end_matches('/')).as_str())?;
        let graphql_url = Url::parse(
            env::var("GITHUB_GRAPHQL_URL")
                .unwrap_or("https://api.github.com/graphql".to_string())
                .as_str(),
        )?;

        Ok(Self {
            client: Client::builder()
                .default_headers(Self::make_headers()?)
                .user_agent(USER_AGENT)
                .build()?,
            pull_request,
            event_name,
            api_url,
            graphql_url,
            repo: env::var("GITHUB_REPOSITORY")
                .map_err(|e| ClientError::env_var("GITHUB_REPOSITORY", e))?,
            sha: env::var("GITHUB_SHA").map_err(|e| ClientError::env_var("GITHUB_SHA", e))?,
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
        let repo = format!(
            "repos/{}{}/comments",
            // if we got here, then we know it is on a CI runner as self.repo should be known
            self.repo,
            if self.is_pr_event() { "/issues" } else { "" },
        );
        let base_comment_url = self.api_url.join(&repo)?;
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
                        serde_json::from_str::<Vec<ThreadComment>>(&response.text().await?)
                            .map_err(|e| {
                                ClientError::json("deserialize list of existing thread comments", e)
                            })?;
                    for comment in payload {
                        if comment.body.starts_with(comment_marker) {
                            log::debug!(
                                "Found bot comment id {} from user {} ({})",
                                comment.id,
                                comment.user.login,
                                comment.user.id,
                            );
                            let this_comment_url =
                                Url::parse(format!("{base_comment_url}/{}", comment.id).as_str())?;
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
                                    del_url.to_owned(),
                                    Method::DELETE,
                                    None,
                                    None,
                                )?;
                                let result = self
                                    .send_api_request(&self.client, req, &self.rate_limit_headers)
                                    .await
                                    .map_err(|e| {
                                        e.add_request_context("delete old thread comment")
                                    })?;
                                self.log_response(result, "Failed to delete old thread comment")
                                    .await;
                            }
                            if !delete {
                                comment_url = Some(this_comment_url)
                            }
                        }
                    }
                }
            }
        }
        Ok(comment_url)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::env;

    use super::GithubApiClient;

    #[test]
    fn ensure_api_url_has_trailing_slash() {
        let gh_api_url = "https://api.github.com/v3";
        unsafe {
            env::set_var("GITHUB_REPOSITORY", "2bndy5/git-bot-feedback");
            env::set_var("GITHUB_SHA", "abc123");
            env::set_var("GITHUB_EVENT_NAME", "push");
            // suffix the URL with `//`; expect `client.api_url` to end with a single `/`
            env::set_var("GITHUB_API_URL", format!("{gh_api_url}//"));
            env::set_var("GITHUB_GRAPHQL_URL", "https://ghe.example.com/api/graphql");
        }
        let client = GithubApiClient::new().unwrap();
        assert_eq!(
            client.graphql_url.as_str(),
            "https://ghe.example.com/api/graphql"
        );
        assert!(client.api_url.as_str().ends_with("/v3/"));
        let endpoint = client.api_url.join("repos/").unwrap();
        assert_eq!(endpoint.as_str(), "https://api.github.com/v3/repos/");
    }

    #[test]
    fn is_pr_same_remote_push() {
        use crate::RestApiClient;

        unsafe {
            env::set_var("GITHUB_REPOSITORY", "2bndy5/git-bot-feedback");
            env::set_var("GITHUB_SHA", "abc123");
            env::set_var("GITHUB_EVENT_NAME", "push");
        }
        let client = GithubApiClient::new().unwrap();
        assert!(!client.is_pr_same_remote());
    }

    #[test]
    fn is_pr_same_remote_same() {
        use crate::RestApiClient;
        use tempfile::NamedTempFile;

        let mut event_file = NamedTempFile::new().unwrap();
        let payload = serde_json::json!({
            "pull_request": {
                "draft": false,
                "state": "open",
                "number": 42,
                "locked": false,
                "head": {"repo": {"id": 100, "full_name": "owner/repo"}},
                "base": {"repo": {"id": 100, "full_name": "owner/repo"}}
            }
        });
        std::io::Write::write_all(&mut event_file, payload.to_string().as_bytes()).unwrap();
        unsafe {
            env::set_var("GITHUB_REPOSITORY", "2bndy5/git-bot-feedback");
            env::set_var("GITHUB_SHA", "abc123");
            env::set_var("GITHUB_EVENT_NAME", "pull_request");
            env::set_var("GITHUB_EVENT_PATH", event_file.path());
        }
        let client = GithubApiClient::new().unwrap();
        assert!(client.is_pr_same_remote());
    }

    #[test]
    fn is_pr_same_remote_fork() {
        use crate::RestApiClient;
        use tempfile::NamedTempFile;

        let mut fork_event_file = NamedTempFile::new().unwrap();
        let fork_payload = serde_json::json!({
            "pull_request": {
                "draft": false,
                "state": "open",
                "number": 42,
                "locked": false,
                "head": {"repo": {"id": 200, "full_name": "fork/repo"}},
                "base": {"repo": {"id": 100, "full_name": "owner/repo"}}
            }
        });
        std::io::Write::write_all(&mut fork_event_file, fork_payload.to_string().as_bytes())
            .unwrap();
        unsafe {
            env::set_var("GITHUB_REPOSITORY", "2bndy5/git-bot-feedback");
            env::set_var("GITHUB_SHA", "abc123");
            env::set_var("GITHUB_EVENT_NAME", "pull_request");
            env::set_var("GITHUB_EVENT_PATH", fork_event_file.path());
        }
        let client = GithubApiClient::new().unwrap();
        assert!(!client.is_pr_same_remote());
    }
}
