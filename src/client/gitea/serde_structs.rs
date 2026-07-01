//! This submodule declares data structures used to
//! deserialize (and serializer) JSON payload data.

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Serialize, PartialEq, Clone)]
pub struct FullReview {
    pub body: String,
    pub comments: Vec<ReviewDiffComment>,
    pub commit_id: String,
    pub event: String,
}

#[derive(Debug, Serialize, PartialEq, Clone)]
pub struct ReviewDiffComment {
    pub body: String,
    pub new_position: i64,
    pub old_position: i64,
    pub path: String,
}

impl From<&crate::ReviewComment> for ReviewDiffComment {
    fn from(comment: &crate::ReviewComment) -> Self {
        Self {
            body: comment.comment.clone(),
            new_position: comment.line_end as i64,
            old_position: comment.line_start.map(|i| i as i64).unwrap_or_default(),
            path: comment.path.clone(),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewState {
    /// The review is an approval.
    Approved,
    /// The review is pending submission.
    Pending,
    /// The review is a comment.
    Comment,
    /// The review is a request for changes.
    RequestChanges,
    /// The review is a request for additional review.
    RequestReview,
}

/// A structure for deserializing a Gitea pull request review from a response's json.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct ReviewInfo {
    /// The review's ID number.
    pub id: i64,
    /// The review's body/summary comment.
    pub body: String,
    /// The review's author.
    pub user: User,
    /// The review's state (e.g. "PENDING", "APPROVED", "REQUEST_CHANGES", "COMMENTED").
    pub state: ReviewState,
    /// The review's comments on specific diff lines.
    #[serde(default)]
    pub comments: Vec<GiteaReviewComment>,
    /// The number of comments in this review.
    pub comments_count: i64,
}

/// A structure for deserializing a Gitea review comment from a response's json.
///
/// These are comments on specific lines in the diff, not the review summary.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct GiteaReviewComment {
    /// The comment's ID number.
    pub id: i64,
    /// The file path this comment is on.
    pub path: String,
    /// The position in the old file (for deleted/modified lines).
    #[serde(default)]
    pub old_position: i64,
    /// The position in the new file (for added/modified lines).
    #[serde(default)]
    pub new_position: i64,
    /// The comment body text.
    pub body: String,
}
