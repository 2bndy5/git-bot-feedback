use std::collections::{HashMap, HashSet};

use reqwest::{Method, Url};
use serde::Deserialize;
use serde_json::json;

use crate::{
    RestClientError, ReviewOptions,
    client::{RestApiClient, github::serde_structs::ReviewState, send_api_request},
};

use super::{GithubApiClient, serde_structs::ReviewComment};

const QUERY_REVIEW_COMMENTS: &str = r#"query($owner: String!, $name: String!, $number: Int!, $afterThread: String, $afterComment: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      reviewThreads(last: 100, after: $afterThread) {
        nodes {
          id
          isResolved
          isCollapsed
          isOutdated
          comments(first: 100, after: $afterComment) {
            nodes {
              id
              body
              path
              line
              startLine
              originalLine
              originalStartLine
              pullRequestReview {
                id
                isMinimized
              }
            }
            pageInfo {
              endCursor
              hasNextPage
            }
          }
        }
        pageInfo {
          endCursor
          hasNextPage
        }
      }
    }
  }
}"#;

const RESOLVE_REVIEW_COMMENT: &str = r#"mutation($id: ID!) {
  resolveReviewThread(input: {threadId: $id, clientMutationId: "github-actions"}) {
    thread {
      id
    }
  }
}"#;

const DELETE_REVIEW_COMMENT: &str = r#"mutation($id: ID!) {
  deletePullRequestReviewComment(input: {id: $id, clientMutationId: "github-actions"}) {
    pullRequestReviewComment {
      id
    }
  }
}"#;

const HIDE_REVIEW_COMMENT: &str = r#"mutation($subjectId: ID!) {
  minimizeComment(input: {classifier:OUTDATED, subjectId: $subjectId, clientMutationId: "github-actions"}) {
    minimizedComment {
      isMinimized
    }
  }
}"#;

/// A constant string used as a payload to dismiss PR reviews.
pub const REVIEW_DISMISSAL: &str = r#"{"event":"DISMISS","message":"outdated review"}"#;

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct ThreadInfo {
    pub id: String,
    pub is_collapsed: bool,
    pub is_resolved: bool,
    pub is_outdated: bool,
}

impl From<&QueryResponseReviewThread> for ThreadInfo {
    fn from(thread: &QueryResponseReviewThread) -> Self {
        Self {
            id: thread.id.clone(),
            is_collapsed: thread.is_collapsed,
            is_resolved: thread.is_resolved,
            is_outdated: thread.is_outdated,
        }
    }
}

pub struct ReviewThread {
    pub info: ThreadInfo,
    pub comments: Vec<QueryResponseReviewThreadComment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponsePrReview {
    pub id: String,
    pub is_minimized: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponseReviewThreadComment {
    pub id: String,
    pub body: String,
    pub path: String,
    pub line: Option<i64>,
    pub start_line: Option<i64>,
    pub original_line: i64,
    pub original_start_line: Option<i64>,
    pub pull_request_review: QueryResponsePrReview,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponseReviewThreadComments {
    pub nodes: Vec<QueryResponseReviewThreadComment>,
    page_info: PageInfo,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponseReviewThread {
    pub id: String,
    pub is_collapsed: bool,
    pub is_resolved: bool,
    pub is_outdated: bool,
    pub comments: QueryResponseReviewThreadComments,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryResponseReviewThreads {
    nodes: Vec<QueryResponseReviewThread>,
    page_info: PageInfo,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryResponsePr {
    review_threads: QueryResponseReviewThreads,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryResponseRepo {
    pull_request: QueryResponsePr,
}

#[derive(Debug, Deserialize)]
struct QueryResponseData {
    repository: QueryResponseRepo,
}

#[derive(Debug, Deserialize)]
struct QueryResponse {
    pub data: QueryResponseData,
}

impl GithubApiClient {
    /// Creates the list existing review thread comments to close.
    ///
    /// Set `no_dismissed` is `true` to ignore any already dismissed comments.
    pub(super) async fn get_existing_review_comments(
        &self,
        marker: &str,
        no_dismissed: bool,
    ) -> Result<Vec<ReviewThread>, RestClientError> {
        let mut found_threads: HashMap<ThreadInfo, Vec<QueryResponseReviewThreadComment>> =
            HashMap::new();
        // We should never reach the `default_value` in `.unwrap_or(default_value)` because
        // the repo name should always have a `/` to delimit the repo's owner and name.
        let (repo_owner, repo_name) = self.repo.split_once('/').unwrap_or(("", ""));
        let mut after_thread = None;
        let mut after_comment = None;
        let mut has_next_page = true;
        let graphql_url = self.api_url.join("/graphql")?;
        while has_next_page {
            let variables = json!({
                "owner": repo_owner.to_string(),
                "name": repo_name.to_string(),
                "number": self.pull_request,
                "afterThread": after_thread,
                "afterComment": after_comment,
            });
            let req = Self::make_api_request(
                &self.client,
                graphql_url.clone(),
                Method::POST,
                Some(json!({"query": QUERY_REVIEW_COMMENTS, "variables": variables}).to_string()),
                None,
            )?;
            let response = send_api_request(&self.client, req, &self.rate_limit_headers).await;
            match response {
                Err(_) => {
                    log::error!("Failed to get existing review thread comments");
                    break;
                }
                Ok(response) => {
                    let text = response.text().await?;
                    match serde_json::from_str::<QueryResponse>(&text) {
                        Err(e) => {
                            log::error!(
                                "GraphQL response was malformed. Failed to deserialize payload: {e}. {text}"
                            );
                            break;
                        }
                        Ok(payload) => {
                            let threads_data = payload.data.repository.pull_request.review_threads;
                            let thread_pg_info = threads_data.page_info;
                            for thread in threads_data.nodes {
                                let comment_data = &thread.comments;
                                let comment_pg_info = &comment_data.page_info;
                                let thread_info = ThreadInfo::from(&thread);
                                for comment in &comment_data.nodes {
                                    if comment.body.starts_with(marker)
                                        && (!no_dismissed
                                            || (!thread.is_resolved && !thread.is_collapsed))
                                    {
                                        if let Some(item) = found_threads.get_mut(&thread_info) {
                                            item.push(comment.clone());
                                        } else {
                                            found_threads
                                                .insert(thread_info.clone(), vec![comment.clone()]);
                                        }
                                    }
                                }
                                after_comment = if comment_pg_info.has_next_page {
                                    comment_pg_info.end_cursor.clone()
                                } else {
                                    None
                                };
                            }
                            if after_comment.is_none() {
                                if !thread_pg_info.has_next_page {
                                    has_next_page = false;
                                } else {
                                    after_thread = thread_pg_info.end_cursor;
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut result = vec![];
        for (info, comments) in found_threads {
            result.push(ReviewThread { info, comments });
        }
        Ok(result)
    }

    /// This will sort through the threads of PR reviews and return a list of
    /// bot comments to be kept.
    ///
    /// This will also resolve (or delete if `delete_review_comments` is `true`)
    /// any outdated unresolved comment.
    ///
    /// The returned list of strings are the IDs (as used in graphQL API) of
    /// the PR reviews that should be kept.
    pub(super) async fn check_reused_comments(
        &self,
        options: &mut ReviewOptions,
        delete_review_comments: bool,
    ) -> Result<Vec<String>, RestClientError> {
        let mut reused_reviews = vec![];
        let found_threads = self
            .get_existing_review_comments(&options.marker, !delete_review_comments)
            .await?;
        if found_threads.is_empty() {
            return Ok(reused_reviews);
        }

        // Keep already posted comments if they match new ones
        let mut existing_review_comments = HashSet::new();
        for thread in &found_threads {
            let mut keep_thread = false; // should we `keep` the whole thread?
            for comment in &thread.comments {
                let line_start = comment
                    .start_line
                    .unwrap_or(comment.original_start_line.unwrap_or(-1));
                let line_end = comment.line.unwrap_or(comment.original_line);
                let mut keep = false; // should we `keep` the review comment?
                for suggestion in &options.comments {
                    if suggestion.path == comment.path
                        && suggestion.line_start as i64 == line_start
                        && suggestion.line_end as i64 == line_end
                        && format!("{}{}", options.marker, suggestion.comment) == comment.body
                        && !thread.info.is_resolved
                        && !thread.info.is_collapsed
                        && !thread.info.is_outdated
                        && !comment.pull_request_review.is_minimized
                    {
                        log::info!(
                            "Using existing review comment: path='{}', line_start='{line_start}', line_end='{line_end}'",
                            comment.path,
                        );
                        reused_reviews.push(comment.pull_request_review.id.clone());
                        existing_review_comments.insert(suggestion.clone());
                        keep = true;
                        keep_thread = true;
                        break;
                    }
                }
                if !keep {
                    self.close_review_comment(comment.id.clone(), delete_review_comments)
                        .await?;
                }
            }
            if !keep_thread {
                // We don't delete the whole thread since there may be other non-bot comments in the thread.
                // Instead, we'll just mark it as resolved (effectively hiding/collapsing it).
                self.close_review_comment(thread.info.id.clone(), false)
                    .await?;
            }
        }
        options
            .comments
            .retain(|c| !existing_review_comments.contains(c));
        Ok(reused_reviews)
    }

    /// Resolve or Delete an existing review thread comment.
    ///
    /// Pass a thread `id` to resolve/delete the entire thread.
    /// A thread is a conversation focused on a single part of the diff.
    ///
    /// Pass a comment `id` to resolve/delete a specific comment within the thread.
    ///
    /// Pass `delete` as `true` to delete the review comment/thread, `false` to set it as resolved.
    /// Typically, it is undesirable to delete a thread since there may be other non-bot comments in the thread.
    async fn close_review_comment(&self, id: String, delete: bool) -> Result<(), RestClientError> {
        let mutation = if delete {
            DELETE_REVIEW_COMMENT
        } else {
            RESOLVE_REVIEW_COMMENT
        };
        let request = Self::make_api_request(
            &self.client,
            self.api_url.join("/graphql")?,
            Method::POST,
            Some(json!({"query": mutation, "variables": { "id": id }}).to_string()),
            None,
        )?;
        if let Ok(response) =
            send_api_request(&self.client, request, &self.rate_limit_headers).await
        {
            let success = response.status().is_success();
            let op = if delete { "Delete" } else { "Resolve" };
            log::debug!(
                "{op} review comment {} ({}: {id})",
                if success { "failed" } else { "succeeded" },
                if delete { "comment_id" } else { "thread_id" },
            );
            if !success {
                log::error!("GraphQL '{op}' response: {}", response.text().await?);
            }
        }
        Ok(())
    }

    /// Hide and dismiss review that were previously created by this software.
    ///
    /// The `keep_reviews` parameter is a list of reviews' Node IDs to keep displayed.
    /// This also will dismiss any review (as "outdated") if it is not being kept.
    pub(super) async fn hide_outdated_reviews(
        &self,
        url: Url,
        keep_reviews: Vec<String>,
        marker: &str,
    ) -> Result<(), RestClientError> {
        let mut next_page = Some(Url::parse_with_params(url.as_str(), [("page", "1")])?);
        let graphql_url = self.api_url.join("/graphql")?;
        while let Some(url) = next_page {
            let request =
                Self::make_api_request(&self.client, url.clone(), Method::GET, None, None)?;
            let response = send_api_request(&self.client, request, &self.rate_limit_headers).await;
            match response {
                Err(_) => {
                    log::warn!("Failed to get list of existing reviews");
                    return Ok(());
                }
                Ok(response) => {
                    next_page = Self::try_next_page(response.headers());
                    let reviews = serde_json::from_str::<Vec<ReviewComment>>(
                        response.text().await?.as_str(),
                    )?;
                    for review in reviews {
                        if keep_reviews.contains(&review.node_id)
                            || review.body.as_ref().is_none_or(|b| !b.starts_with(marker))
                        {
                            // if the review is being reused or is not authored by this software, then
                            // leave it as is and skip to the next review.
                            continue;
                        }
                        let req = Self::make_api_request(
                            &self.client,
                            graphql_url.clone(),
                            Method::POST,
                            Some(json!({"query": HIDE_REVIEW_COMMENT, "variables": {"subjectId": review.node_id}}).to_string()),
                            None
                        )?;
                        if let Ok(res) =
                            send_api_request(&self.client, req, &self.rate_limit_headers).await
                        {
                            log::debug!(
                                "Minimized review comment: {} (node_id: {})",
                                res.status().is_success(),
                                review.node_id,
                            )
                        }
                        if review.state != ReviewState::Dismissed
                            && let Ok(dismiss_url) =
                                url.join(format!("reviews/{}/dismissals", review.id).as_str())
                            && let Ok(req) = Self::make_api_request(
                                &self.client,
                                dismiss_url,
                                Method::PUT,
                                Some(REVIEW_DISMISSAL.to_string()),
                                None,
                            )
                        {
                            match send_api_request(&self.client, req, &self.rate_limit_headers)
                                .await
                            {
                                Ok(result) => {
                                    if !result.status().is_success() {
                                        Self::log_response(
                                            result,
                                            "Failed to dismiss outdated review",
                                        )
                                        .await;
                                    }
                                }
                                Err(e) => {
                                    log::error!("Failed to dismiss outdated review: {e:}");
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
