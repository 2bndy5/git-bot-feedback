use std::sync::Arc;
use tokio::sync::Mutex;

use ::git_bot_feedback::client::{RestApiClient, init_client};
use pyo3::prelude::*;

#[pyclass(module = "git_bot_feedback")]
pub struct GitClient {
    client: Arc<Mutex<Box<dyn RestApiClient + Send + Sync>>>,
}

#[pymethods]
impl GitClient {
    #[new]
    pub fn new() -> PyResult<Self> {
        let client = init_client()?;
        Ok(GitClient {
            client: Arc::new(Mutex::new(client)),
        })
    }

    /// Is the current CI event **trigger** a Pull Request?
    ///
    /// This **will not** check if a push event's instigating commit is part of any PR.
    pub fn is_pr_event(&self) -> bool {
        self.client.blocking_lock().is_pr_event()
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
        self.client.blocking_lock().set_user_agent(user_agent)?;
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
                .await?;
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
                .await?;
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
        self.client.blocking_lock().append_step_summary(comment)?;
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
                .await?;
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
            client_clone.lock().await.post_pr_review(&options).await?;
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
        self.client.blocking_lock().write_output_variables(&vars)?;
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
            .write_file_annotations(&annotations)?;
        Ok(())
    }

    // /// Construct a HTTP request to be sent.
    // ///
    // /// The idea here is that this method is called before [`Self::send_api_request()`].
    // /// ```ignore
    // /// let request = Self::make_api_request(
    // ///     &self.client,
    // ///     Url::parse("https://example.com").unwrap(),
    // ///     Method::GET,
    // ///     None,
    // ///     None,
    // /// ).unwrap();
    // /// let response = send_api_request(&self.client, request, &self.rest_api_headers);
    // /// match response.await {
    // ///     Ok(res) => todo!(handle response),
    // ///     Err(e) => todo!(handle failure),
    // /// }
    // /// ```
    // fn make_api_request(
    //     &self,
    //     url: &str,
    //     method: &str,
    //     data: Option<String>,
    //     headers: Option<HeaderMap>,
    // ) -> PyResult<Request> {
    //     let url = Url::parse(url).map_err(|e| {
    //         pyo3::exceptions::PyValueError::new_err(format!("Invalid URL '{url}': {e}"))
    //     })?;
    //     self.client.make_api_request(client, url, method, data, headers)
    // }

    // /// A convenience function to send HTTP requests and respect a REST API rate limits.
    // ///
    // /// This method respects both primary and secondary rate limits.
    // /// In the event where the secondary rate limits is reached,
    // /// this function will wait for a time interval (if specified by the server) and retry afterward.
    // pub fn send_api_request<'py>(
    //     &self,
    //     py: Python<'py>,
    //     client: &Client,
    //     request: Request,
    //     rate_limit_headers: &RestApiRateLimitHeaders,
    // ) -> PyResult<Bound<'py, PyAny>> {
    //     pyo3_async_runtimes::tokio::future_into_py(py, async move {
    //         let result = self
    //             .client
    //             .send_api_request(client, request, rate_limit_headers)
    //             .await?;
    //         Ok(result)
    //     })
    // }

    // /// Gets the URL for the next page from the headers in a paginated response.
    // ///
    // /// Returns [`None`] if current response is the last page.
    // fn try_next_page(&self, headers: HashMap<String, String>) -> Option<String> {
    //     let header_map = headers
    //         .into_iter()
    //         .filter_map(|(k, v)| {
    //             if let Some(val) = reqwest::header::HeaderValue::from_str(&v).ok()
    //                 && let Some(name) = reqwest::header::HeaderName::from_str(&k).ok()
    //             {
    //                 Some((name, val))
    //             } else {
    //                 None
    //             }
    //         })
    //         .collect::<reqwest::header::HeaderMap>();
    //     self.client
    //         .blocking_lock()
    //         .try_next_page(&header_map)
    //         .map(|u| u.to_string())
    // }

    // /// A helper function to log the response of an API request with context.
    // ///
    // /// This also dumps the response body as text if possible.
    // pub async fn log_response<'py>(&self, py: Python<'py>, response: Response, context: &str) {
    //     if let Err(e) = response.error_for_status_ref() {
    //         log::error!("{}: {e:?}", context.to_owned());
    //         if let Ok(text) = response.text().await {
    //             log::error!("{text}");
    //         }
    //     }
    // }

    /// The name of git server implementation is being used.
    #[getter]
    pub fn client_kind(&self) -> String {
        self.client.blocking_lock().client_kind()
    }
}
