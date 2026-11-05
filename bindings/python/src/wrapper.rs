use std::{collections::HashMap, path::PathBuf, str::FromStr, sync::Arc};
use tokio::sync::Mutex;

use ::git_bot_feedback::{
    RestClientError,
    client::{RestApiClient, USER_AGENT, init_client},
};
use pyo3::prelude::*;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::py_types::{PyRequest, PyResponse, rate_limit_headers, to_py_err};

#[pyclass(module = "git_bot_feedback")]
pub struct GitClient {
    client: Arc<Mutex<Box<dyn RestApiClient + Send + Sync>>>,
    /// The HTTP client used by [`Self::send_api_request()`].
    http: std::sync::Mutex<reqwest::Client>,
}

impl GitClient {
    fn http(&self) -> reqwest::Client {
        self.http.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[pymethods]
impl GitClient {
    #[new]
    pub fn new() -> PyResult<Self> {
        let client = init_client().map_err(to_py_err)?;
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| to_py_err(RestClientError::Request(e)))?;
        Ok(GitClient {
            client: Arc::new(Mutex::new(client)),
            http: std::sync::Mutex::new(http),
        })
    }

    /// Is the current CI event **trigger** a Pull Request?
    ///
    /// This **will not** check if a push event's instigating commit is part of any PR.
    pub fn is_pr_event(&self) -> bool {
        self.client.blocking_lock().is_pr_event()
    }

    /// Is the Pull Request event originating from the same remote repository?
    ///
    /// Returns `false` if the event is not a Pull Request event, or if the
    /// Pull Request originates from a fork repository.
    pub fn is_pr_same_remote(&self) -> bool {
        self.client.blocking_lock().is_pr_same_remote()
    }

    /// Is debug mode enabled?
    ///
    /// Typically, A CI platform will have a way to enable debug level logs for a job or workflow.
    /// This method should be implemented to reflect the supported CI platform's implementation.
    fn is_debug_enabled(&self) -> bool {
        self.client.blocking_lock().is_debug_enabled()
    }

    /// Get the name of the current CI event.
    ///
    /// This will return ``None`` if the event name is not known for the CI platform.
    #[getter]
    fn event_name(&self) -> Option<String> {
        self.client.blocking_lock().event_name()
    }

    /// Set the user agent for the underlying HTTP request client.
    ///
    /// By default the user agent is set to this lib's name and version.
    /// See [`USER_AGENT`] for the default value.
    pub fn set_user_agent(&self, user_agent: &str) -> PyResult<()> {
        self.client
            .blocking_lock()
            .set_user_agent(user_agent)
            .map_err(to_py_err)?;
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .build()
            .map_err(|e| to_py_err(RestClientError::Request(e)))?;
        *self.http.lock().unwrap_or_else(|e| e.into_inner()) = http;
        Ok(())
    }

    /// A way to get the list of changed files in the context of the CI event.
    ///
    /// This method will parse diff blobs and return a list of changed files.
    ///
    /// The default implementation uses `git diff` to get the list of changed files.
    /// So, the default implementation requires `git` installed and a non-shallow checkout.
    ///
    /// Other implementations use the Git server's REST API to get the list of changed files.
    #[pyo3(
        signature = (file_filter, lines_changed_only, base_diff=None, ignore_index=false),
        text_signature = "(file_filter: FileFilter, lines_changed_only: LinesChangedOnly, base_diff: str | None = None, ignore_index: bool = False) -> dict[str, FileDiffLines]"
    )]
    pub fn get_list_of_changed_files<'py>(
        &self,
        py: Python<'py>,
        file_filter: &::git_bot_feedback::FileFilter,
        lines_changed_only: ::git_bot_feedback::LinesChangedOnly,
        base_diff: Option<String>,
        ignore_index: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client_clone = Arc::clone(&self.client);
        let file_filter = file_filter.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let result = client_clone
                .lock()
                .await
                .get_list_of_changed_files(
                    &file_filter,
                    &lines_changed_only,
                    base_diff,
                    ignore_index,
                )
                .await
                .map_err(to_py_err)?;
            Ok(result)
        })
    }

    /// A way to post feedback to the Git server's GUI.
    ///
    /// The given [`ThreadCommentOptions::comment`] should be compliant with
    /// the Git server's requirements (ie. the comment length is within acceptable limits).
    #[pyo3(
        signature = (options),
        text_signature = "(options: ThreadCommentOptions) -> None"
    )]
    pub fn post_thread_comment<'py>(
        &self,
        py: Python<'py>,
        options: ::git_bot_feedback::ThreadCommentOptions,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client_clone = Arc::clone(&self.client);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client_clone
                .lock()
                .await
                .post_thread_comment(options)
                .await
                .map_err(to_py_err)?;
            Ok(())
        })
    }

    /// Appends a given comment to the CI workflow's summary page.
    ///
    /// This is the least obtrusive and recommended for push events.
    /// Not all Git servers natively support this type of feedback.
    /// GitHub and Gitea are known to support this.
    /// For all other git servers, this is a non-op returning [`Ok`]
    #[pyo3(
        signature = (comment),
        text_signature = "(comment: str) -> None"
    )]
    pub fn append_step_summary(&self, comment: &str) -> PyResult<()> {
        self.client
            .blocking_lock()
            .append_step_summary(comment)
            .map_err(to_py_err)?;
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
    ///
    /// Returns a copy of the given ``ReviewOptions`` with duplicate comments removed.
    #[pyo3(
        signature = (options),
        text_signature = "(options: ReviewOptions) -> ReviewOptions"
    )]
    pub fn cull_pr_reviews<'py>(
        &self,
        py: Python<'py>,
        mut options: ::git_bot_feedback::ReviewOptions,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client_clone = Arc::clone(&self.client);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client_clone
                .lock()
                .await
                .cull_pr_reviews(&mut options)
                .await
                .map_err(to_py_err)?;
            Ok(options)
        })
    }

    /// Post a PR review based on the given options.
    ///
    /// This is expected to be used after calling [`Self::cull_pr_reviews()`] to
    /// avoid posting duplicates of existing comments. Once the duplicates are filtered out,
    /// the [`ReviewOptions::summary`] can be made to reflect the actual review being posted.
    ///
    /// This function does nothing for non-PR events.
    #[pyo3(
        signature = (options),
        text_signature = "(options: ReviewOptions) -> None"
    )]
    pub fn post_pr_review<'py>(
        &self,
        py: Python<'py>,
        options: ::git_bot_feedback::ReviewOptions,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client_clone = Arc::clone(&self.client);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client_clone
                .lock()
                .await
                .post_pr_review(&options)
                .await
                .map_err(to_py_err)?;
            Ok(())
        })
    }

    /// Sets the given `vars` as output variables.
    ///
    /// These variables are designed to be consumed by other steps in the CI workflow.
    #[pyo3(
        signature = (vars),
        text_signature = "(vars: list[OutputVariable]) -> None"
    )]
    pub fn write_output_variables(
        &self,
        vars: Vec<::git_bot_feedback::OutputVariable>,
    ) -> PyResult<()> {
        self.client
            .blocking_lock()
            .write_output_variables(&vars)
            .map_err(to_py_err)?;
        Ok(())
    }

    /// Sets the given `annotations` as file annotations.
    ///
    /// Not all Git servers support this on their free tiers, namely GitLab.
    #[pyo3(
        signature = (annotations),
        text_signature = "(annotations: list[FileAnnotation]) -> None"
    )]
    pub fn write_file_annotations(
        &self,
        annotations: Vec<::git_bot_feedback::FileAnnotation>,
    ) -> PyResult<()> {
        self.client
            .blocking_lock()
            .write_file_annotations(&annotations)
            .map_err(to_py_err)?;
        Ok(())
    }

    /// Mark the start of a group of logs statements.
    ///
    /// The name given is used as the unique group name.
    #[pyo3(
        signature = (name),
        text_signature = "(name: str) -> None"
    )]
    pub fn start_log_group(&self, name: &str) {
        self.client.blocking_lock().start_log_group(name);
    }

    /// Mark the end of a group of logs statements.
    ///
    /// Some CI platforms expect the given name to match an already started group of logs.
    /// Other CI platforms do not actually used the given name.
    #[pyo3(
        signature = (name),
        text_signature = "(name: str) -> None"
    )]
    pub fn end_log_group(&self, name: &str) {
        self.client.blocking_lock().end_log_group(name);
    }

    /// Send an HTTP request, respecting the REST API's rate limits.
    ///
    /// The request's headers override the client's default headers.
    /// In the event where a secondary rate limit is reached, this will wait for the
    /// time interval (if specified by the server) and retry afterward.
    /// Raises ``RateLimitError`` or ``RequestError`` on failure.
    #[pyo3(
        signature = (request),
        text_signature = "(request: Request) -> Response"
    )]
    pub fn send_api_request<'py>(
        &self,
        py: Python<'py>,
        request: PyRef<'_, PyRequest>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let request = (*request).clone();
        let client_clone = Arc::clone(&self.client);
        let http = self.http();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = client_clone.lock().await;
            let kind = client.client_kind();
            let parts = request.resolve()?;
            let req = client
                .make_api_request(
                    &http,
                    parts.url,
                    parts.method,
                    parts.body,
                    Some(parts.headers),
                )
                .map_err(to_py_err)?;
            let response = client
                .send_api_request(&http, req, &rate_limit_headers(&kind))
                .await
                .map_err(to_py_err)?;
            PyResponse::from_reqwest(response).await.map_err(to_py_err)
        })
    }

    /// Gets the URL for the next page from the headers in a paginated response.
    ///
    /// Returns ``None`` if the current response is the last page.
    #[pyo3(
        signature = (headers),
        text_signature = "(headers: dict[str, str]) -> str | None"
    )]
    pub fn try_next_page(&self, headers: HashMap<String, String>) -> Option<String> {
        let header_map = headers
            .into_iter()
            .filter_map(|(k, v)| {
                Some((
                    HeaderName::from_str(&k).ok()?,
                    HeaderValue::from_str(&v).ok()?,
                ))
            })
            .collect::<HeaderMap>();
        self.client
            .blocking_lock()
            .try_next_page(&header_map)
            .map(|u| u.to_string())
    }

    /// The name of git server implementation is being used.
    #[getter]
    pub fn client_kind(&self) -> String {
        self.client.blocking_lock().client_kind()
    }

    /// The root URL of the git server's REST API.
    ///
    /// Use this to assemble the ``url`` of a :py:class:`~git_bot_feedback.Request`.
    /// It is ``None`` when not run by a CI platform.
    #[getter]
    pub fn api_url(&self) -> Option<String> {
        self.client.blocking_lock().api_url().map(|u| u.to_string())
    }

    /// The root folder of the workspace, as named by the CI platform's environment
    /// (for example, ``GITHUB_WORKSPACE`` on GitHub).
    ///
    /// Returns ``None`` when not run by a CI platform.
    /// Raises a ``ValueError`` if the CI platform's environment variable cannot be read.
    /// This does not fall back to the working directory or
    /// :py:func:`~git_bot_feedback.nearest_git_root()`.
    pub fn workspace_root(&self) -> PyResult<Option<PathBuf>> {
        self.client
            .blocking_lock()
            .workspace_root()
            .map_err(to_py_err)
    }
}
