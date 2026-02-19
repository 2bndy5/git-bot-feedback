use super::{CommentKind, CommentPolicy, DEFAULT_MARKER};

/// A struct to describe a Pull Request review.
///
/// Each review is considered to be about the PR event's changes.
/// There is no support for posting reviews on older/outdated PR events.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ReviewOptions {
    /// Controls posting comments on a thread that concerns a Pull Request or Push event.
    pub policy: CommentPolicy,

    /// The [`CommentKind`] that describes the comment's purpose.
    pub kind: CommentKind,

    /// The course of action that the PR review suggests.
    pub action: ReviewAction,

    /// A summary of the PR review.
    ///
    /// This is an overview of the review's comments.
    pub summary: String,

    /// A list of comments for changes to the PR.
    pub comments: Vec<GenericReviewComment>,

    /// A string used to mark/identify each comment (and [`Self::summary`]) as a
    /// comment submitted by this software.
    ///
    /// User comments may be indistinguishable from bot/generated comments if
    /// this value is not unique enough.
    ///
    /// If the git server employs Markdown syntax for comments, then
    /// it is recommended to set this to an HTML comment that is unique to
    /// your CI application:
    ///
    /// ```markdown
    /// <!-- my-cool-CI-app-name -->
    /// ```
    ///
    /// The default value for this is an HTML comment generated from
    /// this crate's name and version along with the compile-tome's datetime.
    /// For example:
    ///
    /// ```markdown
    /// <!-- git-bot-feedback/0.1.0/Jul-14-2025_17-00 -->
    /// ```
    pub marker: String,
}

impl Default for ReviewOptions {
    fn default() -> Self {
        Self {
            policy: Default::default(),
            kind: Default::default(),
            action: ReviewAction::Comment,
            summary: Default::default(),
            comments: Default::default(),
            marker: DEFAULT_MARKER.to_string(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ReviewAction {
    Approve,
    RequestChanges,
    Comment,
}

/// A struct to describe a single comment in a Pull Request review.
#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct GenericReviewComment {
    /// The file's line number in the diff that begins the the focus of the comment's concerns.
    pub line_start: u32,

    /// The file's line number in the diff that ends the focus of the comment's concerns.
    pub line_end: u32,

    /// The actual comment.
    ///
    /// This text can include a code block that demonstrates a suggested change(s).
    pub comment: String,

    /// The file that this comment pertains to.
    pub path: String,
}
