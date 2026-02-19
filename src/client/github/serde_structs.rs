//! This submodule declares data structures used to
//! deserialize (and serializer) JSON payload data.

use serde::{Deserialize, Serialize};

use crate::GenericReviewComment;

/// A structure for deserializing a Pull Request's info from a response's json.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct PullRequestInfo {
    /// Is this PR a draft?
    pub draft: bool,
    /// What is current state of this PR?
    ///
    /// Here we only care if it is `"open"`.
    pub state: String,
}

#[derive(Debug, Serialize)]
pub struct FullReview {
    pub event: String,
    pub body: String,
    pub comments: Vec<ReviewDiffComment>,
}

#[derive(Debug, Serialize)]
pub struct ReviewDiffComment {
    pub body: String,
    pub line: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<i64>,
    pub path: String,
}

impl From<&GenericReviewComment> for ReviewDiffComment {
    fn from(comment: &GenericReviewComment) -> Self {
        Self {
            body: comment.comment.clone(),
            line: comment.line_start as i64,
            start_line: Some(comment.line_start as i64),
            path: comment.path.clone(),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
#[serde(rename_all = "UPPERCASE")]
pub enum ReviewState {
    Pending,
    Dismissed,
    Approved,
    Comment,
}

/// A structure for deserializing a comment from a response's json.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct ReviewComment {
    /// The content of the review's summary comment.
    pub body: Option<String>,
    /// The review's ID.
    pub id: i64,
    /// The review's node ID.
    ///
    /// This is really only useful for GraphQL requests.
    pub node_id: String,
    /// The state of the review in question.
    pub state: ReviewState,
}

/// A structure for deserializing a comment from a response's json.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct ThreadComment {
    /// The comment's ID number.
    pub id: i64,
    /// The comment's body number.
    pub body: String,
    /// The comment's user number.
    ///
    /// This is only used for debug output.
    pub user: User,
}

/// A structure for deserializing a comment's author from a response's json.
///
/// This is only used for debug output.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct User {
    pub login: String,
    pub id: u64,
}

/// A structure for deserializing a single changed file in a CI event.
#[cfg(feature = "file-changes")]
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct GithubChangedFile {
    /// The file's name (including relative path to repo root)
    pub filename: String,
    /// If renamed, this will be the file's old name as a [`Some`], otherwise [`None`].
    pub previous_filename: Option<String>,
    /// The individual patch that describes the file's changes.
    pub patch: Option<String>,
    /// The number of changes to the file contents.
    pub changes: i64,
}

/// A structure for deserializing a Push event's changed files.
#[cfg(feature = "file-changes")]
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct PushEventFiles {
    /// The list of changed files.
    pub files: Vec<GithubChangedFile>,
}
