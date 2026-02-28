//! This submodule implements functionality exclusively specific to Github's REST API.

use super::{
    GithubApiClient,
    serde_structs::{PullRequestEventPayload, ThreadComment},
};
use crate::{
    AnnotationLevel, CommentKind, CommentPolicy, FileAnnotation, RestApiClient,
    RestApiRateLimitHeaders, ThreadCommentOptions,
    client::{ClientError, USER_AGENT},
};
use reqwest::{
    Client, Method, Url,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use std::{collections::HashMap, env, fmt::Display, fs};

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
        let api_url = Url::parse(gh_api_url.as_str())?;

        Ok(GithubApiClient {
            client: Client::builder()
                .default_headers(Self::make_headers()?)
                .user_agent(USER_AGENT)
                .build()?,
            pull_request,
            event_name,
            api_url,
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

    fn make_headers() -> Result<HeaderMap<HeaderValue>, ClientError> {
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
        let base_comment_url = self.api_url.join(&repo).unwrap();
        while let Some(ref endpoint) = comments_url {
            let request =
                self.make_api_request(&self.client, endpoint.to_owned(), Method::GET, None, None)?;
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

impl Display for FileAnnotation {
    // here we translate the FileAnnotation struct into the specific string format required by Github Actions for file annotations.
    // See [Github workflow commands documentation](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands#setting-a-debug-message).
    //
    // Example:
    // ::notice file={name},line={line},col={col},endLine={endLine},endColumn={endColumn},title={title}::{message}
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut annotation_str = format!(
            "::{}",
            match self.severity {
                AnnotationLevel::Debug => "debug",
                AnnotationLevel::Notice => "notice",
                AnnotationLevel::Warning => "warning",
                AnnotationLevel::Error => "error",
            }
        );
        if !self.path.is_empty() {
            annotation_str.push_str(" file=");
            annotation_str.push_str(self.path.as_str());
            if let Some(start_line) = self.start_line {
                annotation_str.push_str(format!(",line={start_line}").as_str());
                let col = self.start_column.map(|c| c.max(1));
                if let Some(col) = col {
                    annotation_str.push_str(format!(",col={col}").as_str());
                }
                if let Some(end_line) = self.end_line.map(|l| l.max(1))
                    && end_line > start_line
                {
                    annotation_str.push_str(format!(",endline={end_line}").as_str());
                    if let Some(end_col) = self.end_column.map(|c| c.max(1))
                        && col.is_none_or(|c| c < end_col)
                    {
                        annotation_str.push_str(format!(",endColumn={end_col}").as_str());
                    }
                }
            }
        }
        if let Some(title) = &self.title {
            annotation_str.push_str(",title=");
            annotation_str.push_str(title.as_str());
        }
        write!(f, "{}::{}", annotation_str, self.message)
    }
}

#[cfg(test)]
mod tests {
    use crate::{AnnotationLevel, FileAnnotation};

    #[test]
    fn generic_message() {
        let annotation = FileAnnotation {
            severity: AnnotationLevel::Debug,
            message: "This is a debug message".to_string(),
            ..Default::default()
        };
        assert_eq!(annotation.to_string(), "::debug::This is a debug message");
    }

    #[test]
    fn annotate_file() {
        let annotation = FileAnnotation {
            severity: AnnotationLevel::Warning,
            message: "This is a warning message".to_string(),
            path: "src/main.rs".to_string(),
            title: Some("Warning Title".to_string()),
            ..Default::default()
        };
        assert_eq!(
            annotation.to_string(),
            "::warning file=src/main.rs,title=Warning Title::This is a warning message"
        );
    }

    #[test]
    fn annotate_file_with_start_line() {
        let annotation = FileAnnotation {
            severity: AnnotationLevel::Error,
            path: "src/lib.rs".to_string(),
            message: "This is an error message".to_string(),
            start_line: Some(10),
            ..Default::default()
        };
        assert_eq!(
            annotation.to_string(),
            "::error file=src/lib.rs,line=10::This is an error message"
        );
    }

    #[test]
    fn annotate_file_with_start_line_col() {
        let annotation = FileAnnotation {
            severity: AnnotationLevel::Error,
            path: "src/lib.rs".to_string(),
            message: "This is an error message".to_string(),
            start_line: Some(10),
            start_column: Some(5),
            ..Default::default()
        };
        assert_eq!(
            annotation.to_string(),
            "::error file=src/lib.rs,line=10,col=5::This is an error message"
        );
    }

    #[test]
    fn annotate_file_with_line_span() {
        let annotation = FileAnnotation {
            severity: AnnotationLevel::Notice,
            path: "src/lib.rs".to_string(),
            message: "This is a notice message".to_string(),
            start_line: Some(10),
            end_line: Some(20),
            ..Default::default()
        };
        assert_eq!(
            annotation.to_string(),
            "::notice file=src/lib.rs,line=10,endline=20::This is a notice message"
        );
    }
    #[test]
    fn annotate_file_with_line_col_span() {
        let annotation = FileAnnotation {
            severity: AnnotationLevel::Notice,
            path: "src/lib.rs".to_string(),
            message: "This is a notice message".to_string(),
            start_line: Some(10),
            start_column: Some(5),
            end_line: Some(20),
            end_column: Some(15),
            ..Default::default()
        };
        assert_eq!(
            annotation.to_string(),
            "::notice file=src/lib.rs,line=10,col=5,endline=20,endColumn=15::This is a notice message"
        );
    }
}
