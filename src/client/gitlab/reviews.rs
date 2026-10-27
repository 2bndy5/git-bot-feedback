//! This submodule implements merge request reviews with GitLab's REST API.
//!
//! A review is posted the same way GitLab's web UI submits a review:
//! each comment is created as a draft note, then all draft notes are published at once.
//! This way, only 1 notification is sent for the whole review.

use std::collections::HashSet;

use super::{
    GitlabApiClient, REVIEW_SUMMARY_TAG,
    diff_positions::parse_positions,
    serde_structs::{DiffRefs, Discussion, DraftNote, DraftNotePosition, ExistingDraftNote},
    specific_api::first_page,
};
use crate::{RestApiClient, ReviewAction, ReviewComment, ReviewOptions, client::ClientError};
use reqwest::{Method, Url};
use serde_json::json;

/// Get the body of a review comment as it should be posted on GitLab.
///
/// The `marker` is prepended (if not already).
///
/// A suggestion that spans multiple lines (`line_start` to `line_end`) is anchored on the `line_end`.
/// GitLab expects the number of preceding lines to be specified in the suggestion's code fence
/// (eg. ```` ```suggestion:-2+0 ````); otherwise only the `line_end` would be replaced.
/// So, the offsets are appended to each suggestion's code fence that has none.
/// A code fence that already has offsets (eg. ```` ```suggestion:-1+1 ````) is not changed.
///
/// Like GitLab, carriage returns and trailing whitespace are removed.
/// This way the result can be compared with the body of an existing comment.
/// See [GitLab's note body normalization](https://docs.gitlab.com/api/notes/#note-body-normalization).
pub(super) fn review_comment_body(comment: &ReviewComment, marker: &str) -> String {
    let body = comment.comment.replace('\r', "");
    let body = match comment.line_start {
        Some(start) if start < comment.line_end => {
            let offsets = format!(":-{}+0", comment.line_end - start);
            body.split('\n')
                .map(|line| {
                    if is_suggestion_fence_without_offsets(line) {
                        format!("{}{offsets}", line.trim_end())
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => body,
    };
    if body.starts_with(marker) {
        body.trim_end().to_string()
    } else {
        format!("{marker}{body}").trim_end().to_string()
    }
}

/// Is the given `line` the opening code fence of a suggestion without line offsets?
///
/// For example, ```` ```suggestion ```` or ````` ````suggestion `````,
/// but not ```` ```suggestion:-1+1 ````.
fn is_suggestion_fence_without_offsets(line: &str) -> bool {
    let info = line.trim_start_matches('`');
    line.len() - info.len() >= 3 && info.trim_end() == "suggestion"
}

impl GitlabApiClient {
    /// Get all threads of discussion in a merge request that were started by this software.
    ///
    /// Only threads that start with a note on a line of the diff are included.
    /// Set `no_resolved` to `true` to ignore any already resolved threads.
    async fn get_bot_review_threads(
        &self,
        iid: u64,
        marker: &str,
        no_resolved: bool,
    ) -> Result<Vec<Discussion>, ClientError> {
        let mut threads = vec![];
        let url = self.merge_request_url(iid, "/discussions")?;
        let mut next_page = Some(first_page(url));
        while let Some(endpoint) = next_page.take() {
            let request = self.make_api_request(&self.client, endpoint, Method::GET, None, None)?;
            let response = self
                .send_api_request(&self.client, request, &self.rate_limit_headers)
                .await
                .map_err(|e| e.add_request_context("get existing review comments"))?;
            if !response.status().is_success() {
                self.log_response(response, "Failed to get existing review comments")
                    .await;
                break;
            }
            next_page = self.try_next_page(response.headers());
            let payload = serde_json::from_str::<Vec<Discussion>>(&response.text().await?)
                .map_err(|e| ClientError::json("deserialize existing review comments", e))?;
            for thread in payload {
                if thread
                    .notes
                    .first()
                    .is_some_and(|note| note.position.is_some() && note.body.starts_with(marker))
                    && (!no_resolved || !thread.is_resolved())
                {
                    threads.push(thread);
                }
            }
        }
        Ok(threads)
    }

    /// Remove proposed comments that already exist in previous reviews.
    ///
    /// Any thread started by this software that is not reused is resolved.
    /// Outdated comments are deleted instead if [`ReviewOptions::delete_review_comments`] is `true`.
    pub(super) async fn check_reused_comments(
        &self,
        iid: u64,
        options: &mut ReviewOptions,
    ) -> Result<(), ClientError> {
        let threads = self
            .get_bot_review_threads(iid, &options.marker, !options.delete_review_comments)
            .await?;
        let mut reused_comments = HashSet::new();
        for thread in &threads {
            let resolved = thread.is_resolved();
            let mut keep_thread = false;
            let mut deleted = 0;
            for note in &thread.notes {
                if !note.body.starts_with(&options.marker) {
                    continue;
                }
                let reused = if resolved {
                    None
                } else {
                    options.comments.iter().find(|proposed| {
                        note.position.as_ref().is_some_and(|pos| {
                            pos.new_path.as_ref() == Some(&proposed.path)
                                && pos.new_line == Some(proposed.line_end)
                        }) && note.body == review_comment_body(proposed, &options.marker)
                    })
                };
                if let Some(proposed) = reused {
                    log::info!(
                        "Using existing review comment: path='{}', line_end={}",
                        proposed.path,
                        proposed.line_end
                    );
                    reused_comments.insert(proposed.clone());
                    keep_thread = true;
                } else if options.delete_review_comments {
                    self.delete_review_note(iid, &thread.id, note.id).await?;
                    deleted += 1;
                }
            }
            // Resolve the thread unless it was reused, already resolved, or deleted entirely.
            if !keep_thread && !resolved && deleted < thread.notes.len() {
                self.resolve_thread(iid, &thread.id).await?;
            }
        }
        options.comments.retain(|c| !reused_comments.contains(c));
        Ok(())
    }

    /// Delete a note in a merge request's thread of discussion.
    async fn delete_review_note(
        &self,
        iid: u64,
        thread_id: &str,
        note_id: u64,
    ) -> Result<(), ClientError> {
        let url =
            self.merge_request_url(iid, &format!("/discussions/{thread_id}/notes/{note_id}"))?;
        let request = self.make_api_request(&self.client, url, Method::DELETE, None, None)?;
        let response = self
            .send_api_request(&self.client, request, &self.rate_limit_headers)
            .await
            .map_err(|e| e.add_request_context("delete outdated review comment"))?;
        self.log_response(response, "Failed to delete outdated review comment")
            .await;
        Ok(())
    }

    /// Mark a merge request's thread of discussion as resolved.
    async fn resolve_thread(&self, iid: u64, thread_id: &str) -> Result<(), ClientError> {
        let url = self.merge_request_url(iid, &format!("/discussions/{thread_id}"))?;
        let body = json!({"resolved": true}).to_string();
        let request = self.make_api_request(&self.client, url, Method::PUT, Some(body), None)?;
        let response = self
            .send_api_request(&self.client, request, &self.rate_limit_headers)
            .await
            .map_err(|e| e.add_request_context("resolve outdated review comment"))?;
        self.log_response(response, "Failed to resolve outdated review comment")
            .await;
        Ok(())
    }

    /// Delete pending draft notes that were created by this software but never published.
    ///
    /// Publishing a review publishes all of the user's pending draft notes.
    /// This prevents publishing leftovers from a previous review that failed to be published.
    ///
    /// Returns `true` if the user may have other pending draft notes (eg. a review in progress
    /// when `GITLAB_TOKEN` is a personal access token).
    async fn delete_stale_draft_notes(&self, iid: u64, marker: &str) -> Result<bool, ClientError> {
        let url = self.merge_request_url(iid, "/draft_notes")?;
        let request = self.make_api_request(&self.client, url.clone(), Method::GET, None, None)?;
        let response = self
            .send_api_request(&self.client, request, &self.rate_limit_headers)
            .await
            .map_err(|e| e.add_request_context("get pending draft notes"))?;
        if !response.status().is_success() {
            self.log_response(response, "Failed to get pending draft notes")
                .await;
            return Ok(true);
        }
        let drafts = serde_json::from_str::<Vec<ExistingDraftNote>>(&response.text().await?)
            .map_err(|e| ClientError::json("deserialize pending draft notes", e))?;
        for draft in drafts.iter().filter(|d| d.note.starts_with(marker)) {
            log::debug!("Deleting stale draft note {}", draft.id);
            let del_url = Url::parse(format!("{url}/{}", draft.id).as_str())?;
            let request =
                self.make_api_request(&self.client, del_url, Method::DELETE, None, None)?;
            let response = self
                .send_api_request(&self.client, request, &self.rate_limit_headers)
                .await
                .map_err(|e| e.add_request_context("delete stale draft note"))?;
            self.log_response(response, "Failed to delete stale draft note")
                .await;
        }
        Ok(drafts.iter().any(|d| !d.note.starts_with(marker)))
    }

    /// Create a draft note for each of the given `comments`.
    ///
    /// Returns the IDs of the draft notes that were created.
    async fn create_draft_notes(
        &self,
        iid: u64,
        diff_refs: &DiffRefs,
        options: &ReviewOptions,
    ) -> Result<Vec<u64>, ClientError> {
        let mut created = vec![];
        if options.comments.is_empty() {
            return Ok(created);
        }
        let positions = parse_positions(&self.get_merge_request_raw_diff(iid).await?)?;
        let url = self.merge_request_url(iid, "/draft_notes")?;
        for comment in &options.comments {
            let Some(file) = positions.get(&comment.path) else {
                log::warn!(
                    "Skipping review comment for {}:{} because the file is not in the diff",
                    comment.path,
                    comment.line_end
                );
                continue;
            };
            let payload = DraftNote {
                note: review_comment_body(comment, &options.marker),
                position: DraftNotePosition {
                    diff_refs: diff_refs.clone(),
                    position_type: "text",
                    old_path: file.old_path.clone(),
                    new_path: comment.path.clone(),
                    old_line: file.old_line(comment.line_end),
                    new_line: comment.line_end,
                },
            };
            let body = serde_json::to_string(&payload)
                .map_err(|e| ClientError::json("serialize review comment", e))?;
            let request =
                self.make_api_request(&self.client, url.clone(), Method::POST, Some(body), None)?;
            let response = self
                .send_api_request(&self.client, request, &self.rate_limit_headers)
                .await
                .map_err(|e| e.add_request_context("create review comment"))?;
            if response.status().is_success() {
                let draft = serde_json::from_str::<ExistingDraftNote>(&response.text().await?)
                    .map_err(|e| ClientError::json("deserialize created review comment", e))?;
                created.push(draft.id);
            } else {
                self.log_response(response, "Failed to create review comment")
                    .await;
            }
        }
        Ok(created)
    }

    /// Post a merge request review.
    pub(super) async fn post_review(
        &self,
        iid: u64,
        diff_refs: &DiffRefs,
        options: &ReviewOptions,
    ) -> Result<(), ClientError> {
        let has_other_drafts = self.delete_stale_draft_notes(iid, &options.marker).await?;
        let drafts = self.create_draft_notes(iid, diff_refs, options).await?;
        let request_changes = options.action == ReviewAction::RequestChanges;
        if has_other_drafts {
            // Publishing all draft notes would also publish the user's other pending drafts.
            log::warn!(
                "Publishing review comments one at a time because the GitLab user has other pending draft notes on this merge request"
            );
            if request_changes {
                // Only publishing all draft notes can set the reviewer's state.
                log::warn!("The merge request is not marked as needing changes");
            }
            for id in drafts {
                let url = self.merge_request_url(iid, &format!("/draft_notes/{id}/publish"))?;
                self.send_and_log(Method::PUT, url, None, "publish review comment")
                    .await?;
            }
        } else if !drafts.is_empty() || request_changes {
            // `reviewer_state` is ignored by GitLab versions older than v19.2
            let payload = if request_changes {
                json!({"reviewer_state": "requested_changes"})
            } else {
                json!({})
            };
            let url = self.merge_request_url(iid, "/draft_notes/bulk_publish")?;
            self.send_and_log(Method::POST, url, Some(payload), "publish review")
                .await?;
        }

        // A summary note is posted separately because older GitLab versions do not support
        // posting a summary note when publishing a review.
        let summary = format!(
            "{}{REVIEW_SUMMARY_TAG}{}",
            options.marker,
            options
                .summary
                .strip_prefix(&options.marker)
                .unwrap_or(&options.summary)
        );
        let url = self.merge_request_url(iid, "/notes")?;
        self.send_and_log(
            Method::POST,
            url,
            Some(json!({"body": summary})),
            "post review summary",
        )
        .await?;

        match options.action {
            ReviewAction::Approve => {
                let url = self.merge_request_url(iid, "/approve")?;
                self.send_and_log(Method::POST, url, None, "approve merge request")
                    .await?;
            }
            ReviewAction::RequestChanges => {
                // Withdraw a previous approval (if any)
                let url = self.merge_request_url(iid, "/unapprove")?;
                let request = self.make_api_request(&self.client, url, Method::POST, None, None)?;
                let response = self
                    .send_api_request(&self.client, request, &self.rate_limit_headers)
                    .await
                    .map_err(|e| e.add_request_context("withdraw approval"))?;
                if response.status().as_u16() == 404 {
                    log::debug!("Merge request was not previously approved");
                } else {
                    self.log_response(response, "Failed to withdraw approval")
                        .await;
                }
            }
            ReviewAction::Comment => {}
        }
        Ok(())
    }

    /// Send a request (with an optional JSON `payload`) and log the response if it failed.
    async fn send_and_log(
        &self,
        method: Method,
        url: Url,
        payload: Option<serde_json::Value>,
        task: &str,
    ) -> Result<(), ClientError> {
        let body = payload.map(|p| p.to_string());
        let request = self.make_api_request(&self.client, url, method, body, None)?;
        let response = self
            .send_api_request(&self.client, request, &self.rate_limit_headers)
            .await
            .map_err(|e| e.add_request_context(task))?;
        self.log_response(response, format!("Failed to {task}").as_str())
            .await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::review_comment_body;
    use crate::ReviewComment;

    const MARKER: &str = "<!-- marker -->\n";

    fn comment(line_start: Option<u32>, body: &str) -> ReviewComment {
        ReviewComment {
            line_start,
            line_end: 12,
            comment: body.to_string(),
            path: "src/demo.cpp".to_string(),
        }
    }

    #[test]
    fn single_line_suggestion() {
        let body = "Use braces\n```suggestion\nif (x) {\n```\n";
        assert_eq!(
            review_comment_body(&comment(None, body), MARKER),
            format!("{MARKER}Use braces\n```suggestion\nif (x) {{\n```")
        );
    }

    #[test]
    fn multi_line_suggestion() {
        let body = "Use braces\r\n```suggestion\r\nif (x) {\n  y();\n}\n```\n";
        assert_eq!(
            review_comment_body(&comment(Some(10), body), MARKER),
            format!("{MARKER}Use braces\n```suggestion:-2+0\nif (x) {{\n  y();\n}}\n```")
        );
    }

    #[test]
    fn multi_line_suggestion_with_offsets() {
        // The given offsets are kept, even if they differ from `line_start` and `line_end`.
        let body = "Use braces\n```suggestion:-1+1\nif (x) {\n```\n";
        assert_eq!(
            review_comment_body(&comment(Some(10), body), MARKER),
            format!("{MARKER}{body}").trim_end()
        );
    }

    #[test]
    fn multiple_suggestions() {
        // Only the code fence without offsets gets them.
        let body = "Either\n```suggestion:-1+0\na\n```\nor\n````suggestion \nb\n````";
        assert_eq!(
            review_comment_body(&comment(Some(10), body), MARKER),
            format!("{MARKER}Either\n```suggestion:-1+0\na\n```\nor\n````suggestion:-2+0\nb\n````")
        );
    }

    #[test]
    fn multi_line_comment_without_suggestion() {
        let body = format!("{MARKER}A note about ```suggestion blocks");
        assert_eq!(review_comment_body(&comment(Some(10), &body), MARKER), body);
    }
}
