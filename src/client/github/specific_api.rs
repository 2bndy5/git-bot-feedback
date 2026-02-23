//! This submodule implements functionality exclusively specific to Github's REST API.

use super::{GithubApiClient, serde_structs::ThreadComment};
use crate::{
    CommentKind, CommentPolicy, RestApiClient, RestApiRateLimitHeaders, RestClientError,
    ThreadCommentOptions,
    client::{USER_AGENT, send_api_request},
};
use reqwest::{Client, Method, Url};
use std::{collections::HashMap, env, fs};

type EventPayloadType = serde_json::Map<String, serde_json::Value>;

impl GithubApiClient {
    /// Instantiate a [`GithubApiClient`] object.
    pub fn new() -> Result<Self, RestClientError> {
        let event_name = env::var("GITHUB_EVENT_NAME").unwrap_or(String::from("unknown"));
        let pull_request = {
            match event_name.as_str() {
                "pull_request" => {
                    // GITHUB_*** env vars cannot be overwritten in CI runners on GitHub.
                    let event_payload_path =
                        env::var("GITHUB_EVENT_PATH").map_err(|e| RestClientError::EnvVar {
                            name: "GITHUB_EVENT_PATH".into(),
                            source: e,
                        })?;
                    // event payload JSON file can be overwritten/removed in CI runners
                    let file_buf = fs::read_to_string(event_payload_path.clone()).map_err(|e| {
                        RestClientError::Io {
                            task: format!("read event payload from {event_payload_path}"),
                            source: e,
                        }
                    })?;
                    let payload =
                        serde_json::from_str::<EventPayloadType>(&file_buf).map_err(|e| {
                            RestClientError::Json {
                                task: "deserialize Event Payload".into(),
                                source: e,
                            }
                        })?;
                    payload.get("number").and_then(|v| v.as_i64()).unwrap_or(-1)
                }
                _ => -1,
            }
        };
        // GITHUB_*** env vars cannot be overwritten in CI runners on GitHub.
        let gh_api_url = env::var("GITHUB_API_URL").unwrap_or("https://api.github.com".to_string());
        let api_url = Url::parse(gh_api_url.as_str())?;

        Ok(GithubApiClient {
            client: Client::builder()
                .default_headers(Self::make_headers()?)
                .user_agent(USER_AGENT)
                .build()?,
            pull_request,
            event_name,
            api_url,
            repo: env::var("GITHUB_REPOSITORY").map_err(|e| RestClientError::EnvVar {
                name: "GITHUB_REPOSITORY".into(),
                source: e,
            })?,
            sha: env::var("GITHUB_SHA").map_err(|e| RestClientError::EnvVar {
                name: "GITHUB_SHA".into(),
                source: e,
            })?,
            debug_enabled: env::var("ACTIONS_STEP_DEBUG").is_ok_and(|val| &val == "true"),
            rate_limit_headers: RestApiRateLimitHeaders {
                reset: "x-ratelimit-reset".to_string(),
                remaining: "x-ratelimit-remaining".to_string(),
                retry: "retry-after".to_string(),
            },
        })
    }

    /// Update existing comment or remove old comment(s) and post a new comment
    pub async fn update_comment(
        &self,
        url: Url,
        options: ThreadCommentOptions,
    ) -> Result<(), RestClientError> {
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
            let request = Self::make_api_request(
                &self.client,
                comment_url.unwrap_or(url),
                req_meth,
                Some(serde_json::json!(&payload).to_string()),
                None,
            )?;
            match send_api_request(&self.client, request, &self.rate_limit_headers).await {
                Ok(response) => {
                    Self::log_response(response, "Failed to post thread comment").await;
                }
                Err(e) => {
                    return match e {
                        RestClientError::Request(error) => Err(RestClientError::RequestContext {
                            task: "post thread comment".into(),
                            source: error,
                        }),
                        e => Err(e), // propagate other error variants
                    };
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
    ) -> Result<Option<Url>, RestClientError> {
        let mut comment_url = None;
        let mut comments_url = Some(Url::parse_with_params(url.as_str(), &[("page", "1")])?);
        let repo = format!(
            "repos/{}{}/comments",
            // if we got here, then we know it is on a CI runner as self.repo should be known
            self.repo,
            if self.is_pr_event() { "/issues" } else { "" },
        );
        let base_comment_url = self.api_url.join(&repo).unwrap();
        while let Some(ref endpoint) = comments_url {
            let request =
                Self::make_api_request(&self.client, endpoint.as_str(), Method::GET, None, None)?;
            let result = send_api_request(&self.client, request, &self.rate_limit_headers).await;
            match result {
                Err(e) => {
                    match e {
                        RestClientError::Request(error) => {
                            return Err(RestClientError::RequestContext {
                                task: "get list of existing thread comments".into(),
                                source: error,
                            });
                        }
                        e => return Err(e), // propagate other error variants
                    }
                }
                Ok(response) => {
                    if !response.status().is_success() {
                        Self::log_response(
                            response,
                            "Failed to get list of existing thread comments",
                        )
                        .await;
                        return Ok(comment_url);
                    }
                    comments_url = Self::try_next_page(response.headers());
                    let payload =
                        serde_json::from_str::<Vec<ThreadComment>>(&response.text().await?)
                            .map_err(|e| RestClientError::Json {
                                task: "deserialize list of existing thread comments".into(),
                                source: e,
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
                                let req = Self::make_api_request(
                                    &self.client,
                                    del_url.as_str(),
                                    Method::DELETE,
                                    None,
                                    None,
                                )?;
                                let result =
                                    send_api_request(&self.client, req, &self.rate_limit_headers)
                                        .await
                                        .map_err(|e| {
                                            match e {
                                                RestClientError::Request(error) => {
                                                    RestClientError::RequestContext {
                                                        task: "delete old thread comment".into(),
                                                        source: error,
                                                    }
                                                }
                                                e => e, // propagate other error variants
                                            }
                                        })?;
                                Self::log_response(result, "Failed to delete old thread comment")
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
