//! A module to contain traits and structs that are needed by the rest of the git-bot-feedback crate's API.
use std::{
    env,
    fmt::Debug,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use async_trait::async_trait;
use chrono::DateTime;
use reqwest::{Client, Method, Request, Response, Url, header::HeaderMap};

use crate::{FileAnnotation, OutputVariable, RestClientError, ReviewOptions, ThreadCommentOptions};

#[cfg(feature = "gitea")]
mod gitea;
#[cfg(feature = "gitea")]
pub use gitea::GiteaApiClient;

#[cfg(feature = "github")]
mod github;
#[cfg(feature = "github")]
pub use github::GithubApiClient;

#[cfg(feature = "gitlab")]
mod gitlab;
#[cfg(feature = "gitlab")]
pub use gitlab::GitlabApiClient;

mod local;
pub use local::LocalClient;

mod common;

#[cfg(not(any(
    feature = "github",
    feature = "gitea",
    feature = "gitlab",
    feature = "custom-git-server-impl",
)))]
compile_error!(
    "At least one Git server implementation (eg. 'github') should be enabled via `features`"
);

#[cfg(feature = "file-changes")]
use crate::{FileDiffLines, FileFilter, LinesChangedOnly};
#[cfg(feature = "file-changes")]
use std::collections::HashMap;

/// The User-Agent header value included in all HTTP requests.
pub static USER_AGENT: &str = concat!(env!("CARGO_CRATE_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// A structure to contain the different forms of headers that
/// describe a REST API's rate limit status.
#[derive(Debug, Clone)]
pub struct RestApiRateLimitHeaders {
    /// The header key of the rate limit's reset time.
    pub reset: String,
    /// The header key of the rate limit's remaining attempts.
    pub remaining: String,
    /// The header key of the rate limit's "backoff" time interval.
    pub retry: String,
}

/// Find the closest folder, starting with `path` itself, that has a `.git` folder or file.
///
/// A git submodule has a `.git` file instead of a folder.
/// A relative `path` is resolved against the working directory.
/// The returned path is absolute, with any `.` and `..` components removed,
/// but symbolic links are not resolved.
///
/// Returns [`None`] if no such folder is found.
/// This does not invoke the git CLI.
///
/// ```rust
/// use git_bot_feedback::client::nearest_git_root;
/// use std::path::Path;
///
/// let root = nearest_git_root(Path::new("src")).unwrap();
/// assert!(root.join(".git").exists());
/// ```
pub fn nearest_git_root(path: &Path) -> Option<PathBuf> {
    let absolute = std::path::absolute(path).ok()?;
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component),
        }
    }
    normalized
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
}

/// The [`Result::Err`] type returned for fallible functions in this trait.
pub(crate) type ClientError = RestClientError;

/// The number of attempts made when contending a secondary rate limit in REST API requests.
pub(crate) const MAX_RETRIES: u8 = 5;

/// A custom trait that templates necessary functionality with a Git server's REST API.
#[async_trait]
pub trait RestApiClient {
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
        log::info!(target: "CI_LOG_GROUPING", "start_log_group: {name}");
    }

    /// This prints a line to indicate the ending of a related group of [`log`] statements.
    ///
    /// See also [`RestApiClient::start_log_group`] about special handling of
    /// the log target `"CI_LOG_GROUPING"`.
    fn end_log_group(&self, name: &str) {
        log::info!(target: "CI_LOG_GROUPING", "end_log_group: {name}");
    }

    /// Is the current CI event **trigger** a Pull Request?
    ///
    /// This **will not** check if a push event's instigating commit is part of any PR.
    fn is_pr_event(&self) -> bool;

    /// Is debug mode enabled?
    ///
    /// Typically, A CI platform will have a way to enable debug level logs for a job or workflow.
    /// This method should be implemented to reflect the supported CI platform's implementation.
    fn is_debug_enabled(&self) -> bool {
        false
    }

    /// Get the name of the current CI event.
    ///
    /// This will return [`None`] if the event name is not known for the CI platform.
    fn event_name(&self) -> Option<String> {
        None
    }

    /// The root folder of the workspace, as named by the CI platform's environment
    /// (for example, `GITHUB_WORKSPACE` on GitHub).
    ///
    /// Changed file names reported by the git server's REST API are relative to the
    /// repository's top level, which is this folder unless the repository was checked
    /// out into a sub-folder of it.
    ///
    /// Returns [`Ok(None)`](Option::None) for clients that are not run by a CI platform
    /// (like [`LocalClient`]), and an error if the CI platform's environment variable
    /// cannot be read. This does not fall back to the working directory or
    /// [`nearest_git_root()`]; consumers can choose their own fallback.
    fn workspace_root(&self) -> Result<Option<PathBuf>, ClientError> {
        Ok(None)
    }

    /// Set the user agent for the underlying HTTP request client.
    ///
    /// By default the user agent is set to this lib's name and version.
    /// See [`USER_AGENT`] for the default value.
    fn set_user_agent(&mut self, user_agent: &str) -> Result<(), ClientError>;

    /// A way to get the list of changed files in the context of the CI event.
    ///
    /// This method will parse diff blobs and return a list of changed files.
    ///
    /// The default implementation uses `git diff` to get the list of changed files.
    /// So, the default implementation requires `git` installed and a non-shallow checkout.
    ///
    /// Other implementations use the Git server's REST API to get the list of changed files.
    #[cfg(feature = "file-changes")]
    #[cfg_attr(docsrs, doc(cfg(feature = "file-changes")))]
    async fn get_list_of_changed_files(
        &self,
        file_filter: &FileFilter,
        lines_changed_only: &LinesChangedOnly,
        base_diff: Option<String>,
        ignore_index: bool,
    ) -> Result<HashMap<String, FileDiffLines>, ClientError>;

    /// A way to post feedback to the Git server's GUI.
    ///
    /// The given [`ThreadCommentOptions::comment`] should be compliant with
    /// the Git server's requirements (ie. the comment length is within acceptable limits).
    async fn post_thread_comment(&self, options: ThreadCommentOptions) -> Result<(), ClientError>;

    /// Appends a given comment to the CI workflow's summary page.
    ///
    /// This is the least obtrusive and recommended for push events.
    /// Not all Git servers natively support this type of feedback.
    /// GitHub and Gitea are known to support this.
    /// For all other git servers, this is a non-op returning [`Ok`]
    fn append_step_summary(&self, comment: &str) -> Result<(), ClientError> {
        let _ = comment;
        Ok(())
    }

    /// Resolve outdated PR review comments and remove duplicate/reused comments.
    ///
    /// This should be used before [`Self::post_pr_review()`] to avoid posting duplicates of existing comments.
    /// The [`ReviewOptions::comments`] will be modified to only include comments that should be posted for the current PR review.
    /// After calling this function, the [`ReviewOptions::summary`] can be made to reflect the actual review being posted.
    ///
    /// The [`ReviewOptions::marker`] is used to identify comments from this software.
    /// The [`ReviewOptions::delete_review_comments`] flag will delete outdated review comments.
    /// The [`ReviewOptions::delete_review_comments`] flag does not apply to review summary comments nor
    /// threads of discussion within a review.
    /// A review summary comment will only be hidden/collapsed when all comments in the corresponding
    /// review are resolved.
    ///
    /// This function does nothing for non-PR events.
    async fn cull_pr_reviews(&mut self, options: &mut ReviewOptions) -> Result<(), ClientError>;

    /// Post a PR review based on the given options.
    ///
    /// This is expected to be used after calling [`Self::cull_pr_reviews()`] to
    /// avoid posting duplicates of existing comments. Once the duplicates are filtered out,
    /// the [`ReviewOptions::summary`] can be made to reflect the actual review being posted.
    ///
    /// This function does nothing for non-PR events.
    async fn post_pr_review(&mut self, options: &ReviewOptions) -> Result<(), ClientError>;

    /// Sets the given `vars` as output variables.
    ///
    /// These variables are designed to be consumed by other steps in the CI workflow.
    fn write_output_variables(&self, vars: &[OutputVariable]) -> Result<(), ClientError>;

    /// Sets the given `annotations` as file annotations.
    ///
    /// Not all Git servers support this on their free tiers, namely GitLab.
    fn write_file_annotations(&self, annotations: &[FileAnnotation]) -> Result<(), ClientError> {
        for annotation in annotations {
            log::info!("{annotation:#?}");
        }
        Ok(())
    }

    /// Construct a HTTP request to be sent.
    ///
    /// The idea here is that this method is called before [`Self::send_api_request()`].
    /// ```ignore
    /// let request = Self::make_api_request(
    ///     &self.client,
    ///     Url::parse("https://example.com").unwrap(),
    ///     Method::GET,
    ///     None,
    ///     None,
    /// ).unwrap();
    /// let response = send_api_request(&self.client, request, &self.rest_api_headers);
    /// match response.await {
    ///     Ok(res) => todo!(handle response),
    ///     Err(e) => todo!(handle failure),
    /// }
    /// ```
    fn make_api_request(
        &self,
        client: &Client,
        url: Url,
        method: Method,
        data: Option<String>,
        headers: Option<HeaderMap>,
    ) -> Result<Request, ClientError> {
        let mut req = client.request(method, url);
        if let Some(h) = headers {
            req = req.headers(h);
        }
        if let Some(d) = data {
            req = req.body(d);
        }
        req.build()
            .map_err(|e| ClientError::add_request_context(ClientError::Request(e), "build request"))
    }

    /// A convenience function to send HTTP requests and respect a REST API rate limits.
    ///
    /// This method respects both primary and secondary rate limits.
    /// In the event where the secondary rate limits is reached,
    /// this function will wait for a time interval (if specified by the server) and retry afterward.
    async fn send_api_request(
        &self,
        client: &Client,
        request: Request,
        rate_limit_headers: &RestApiRateLimitHeaders,
    ) -> Result<Response, ClientError> {
        for i in 0..MAX_RETRIES {
            let response = client
                .execute(request.try_clone().ok_or(ClientError::CannotCloneRequest)?)
                .await?;
            if [403u16, 429u16].contains(&response.status().as_u16()) {
                // rate limit may have been exceeded

                // check if primary rate limit was violated
                let mut requests_remaining = None;
                if let Some(remaining) = response.headers().get(&rate_limit_headers.remaining) {
                    requests_remaining = Some(remaining.to_str()?.parse::<i64>()?);
                } else {
                    // NOTE: I guess it is sometimes valid for a response to
                    // not include remaining rate limit attempts
                    log::debug!("Response headers do not include remaining API usage count");
                }
                if requests_remaining.is_some_and(|v| v <= 0) {
                    if let Some(reset_value) = response.headers().get(&rate_limit_headers.reset)
                        && let Some(reset) =
                            DateTime::from_timestamp(reset_value.to_str()?.parse::<i64>()?, 0)
                    {
                        return Err(ClientError::RateLimitPrimary(reset));
                    }
                    return Err(ClientError::RateLimitNoReset);
                }

                // check if secondary rate limit is violated. If so, then backoff and try again.
                if let Some(retry_value) = response.headers().get(&rate_limit_headers.retry) {
                    let interval = Duration::from_secs(
                        retry_value.to_str()?.parse::<u64>()? + (i as u64).pow(2),
                    );
                    #[cfg(feature = "test-skip-wait-for-rate-limit")]
                    {
                        // Output a log statement to use the `interval` variable.
                        log::warn!(
                            "Skipped waiting {} seconds to expedite test",
                            interval.as_secs()
                        );
                    }
                    #[cfg(not(feature = "test-skip-wait-for-rate-limit"))]
                    {
                        tokio::time::sleep(interval).await;
                    }
                    continue;
                }
            }
            return Ok(response);
        }
        Err(ClientError::RateLimitSecondary)
    }

    /// Gets the URL for the next page from the headers in a paginated response.
    ///
    /// Returns [`None`] if current response is the last page.
    fn try_next_page(&self, headers: &HeaderMap) -> Option<Url> {
        if let Some(links) = headers.get("link")
            && let Ok(pg_str) = links.to_str()
        {
            let pages = pg_str.split(", ");
            for page in pages {
                if page.ends_with("; rel=\"next\"") {
                    if let Some(link) = page.split_once(">;") {
                        let url = link.0.trim_start_matches("<").to_string();
                        if let Ok(next) = Url::parse(&url) {
                            return Some(next);
                        } else {
                            log::debug!("Failed to parse next page link from response header");
                        }
                    } else {
                        log::debug!("Response header link for pagination is malformed");
                    }
                }
            }
        }
        None
    }

    /// A helper function to log the response of an API request with context.
    ///
    /// This also dumps the response body as text if possible.
    async fn log_response(&self, response: Response, context: &str) {
        if let Err(e) = response.error_for_status_ref() {
            log::error!("{}: {e:?}", context.to_owned());
            if let Ok(text) = response.text().await {
                log::error!("{text}");
            }
        }
    }

    /// The name of git server implementation is being used.
    fn client_kind(&self) -> String;
}

/// Instantiate an implementation of [`RestApiClient`] based on the environment.
///
/// This will fallback to an instance of [`LocalClient`] if
///
/// - the `GITHUB_ACTIONS` environment variable is not set
/// - the `GITEA_ACTIONS` environment variable is not set
/// - the `FORGEJO_ACTIONS` environment variable is not set
/// - the `GITLAB_CI` environment variable is not set
///
/// Forgejo uses the [`GiteaApiClient`].
/// Forgejo's runner also sets `GITHUB_ACTIONS`, so `FORGEJO_ACTIONS` is checked first.
pub fn init_client() -> Result<Box<dyn RestApiClient + Send + Sync>, ClientError> {
    #[cfg(feature = "gitea")]
    if ["GITEA_ACTIONS", "FORGEJO_ACTIONS"]
        .iter()
        .any(|name| env::var(name).is_ok_and(|v| v.to_lowercase() == "true"))
    {
        return Ok(Box::new(GiteaApiClient::new()?));
    }
    #[cfg(feature = "gitlab")]
    if env::var("GITLAB_CI").is_ok_and(|v| v.to_lowercase() == "true") {
        return Ok(Box::new(GitlabApiClient::new()?));
    }
    #[cfg(feature = "github")]
    if env::var("GITHUB_ACTIONS").is_ok_and(|v| v.to_lowercase() == "true") {
        return Ok(Box::new(GithubApiClient::new()?));
    }
    Ok(Box::new(LocalClient))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::nearest_git_root;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn git_root_of_sub_folder() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(repo.join("member/src")).unwrap();
        let root = nearest_git_root(&repo.join("member/src")).unwrap();
        assert_eq!(root, repo);
        assert_eq!(nearest_git_root(&repo).unwrap(), repo);
        // `.` and `..` are removed before searching
        let indirect = repo.join("member/./src/../src/..");
        assert_eq!(nearest_git_root(&indirect).unwrap(), repo);
    }

    #[test]
    fn git_root_of_submodule() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let submodule = repo.join("libs/sub");
        fs::create_dir_all(&submodule).unwrap();
        // a submodule has a `.git` file
        fs::write(submodule.join(".git"), "").unwrap();
        assert_eq!(nearest_git_root(&submodule.join("src")).unwrap(), submodule);
        assert_eq!(nearest_git_root(&repo.join("libs")).unwrap(), repo);
    }

    #[test]
    fn no_git_root() {
        let tmp = tempdir().unwrap();
        let folder = tmp.path().join("not/a/repo");
        fs::create_dir_all(&folder).unwrap();
        let found = nearest_git_root(&folder);
        // the temp dir is not expected to be inside a git repository
        assert!(found.is_none_or(|root| !root.starts_with(tmp.path())));
    }
}
