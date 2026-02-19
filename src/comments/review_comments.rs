use super::DEFAULT_MARKER;

/// A struct to describe a Pull Request review.
///
/// Each review is considered to be about the PR event's changes.
/// There is no support for posting reviews on older/outdated PR events.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ReviewOptions {
    /// The course of action that the PR review suggests.
    pub action: ReviewAction,

    /// A summary of the PR review.
    ///
    /// This is an overview of the review's comments.
    pub summary: String,

    /// A list of comments for changes to the PR.
    pub comments: Vec<ReviewComment>,

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

    /// Allow posting reviews on draft Pull Requests?
    pub allow_draft: bool,

    /// Allow posting reviews on closed Pull Requests?
    pub allow_closed: bool,

    /// Permanently delete PR review outdated comments instead of hiding them.
    ///
    /// Here be dragons!
    /// Use with extreme caution when asserting this flag.
    /// Setting this flag as `true` will permanently
    /// delete PR review comments that may be pivotal to a thread of discussion.
    ///
    /// Note, this does not apply to PR review summary comments nor threads of
    /// discussion within a review.
    pub delete_review_comments: bool,
}

impl Default for ReviewOptions {
    fn default() -> Self {
        Self {
            action: ReviewAction::default(),
            summary: Default::default(),
            comments: Default::default(),
            marker: DEFAULT_MARKER.to_string(),
            allow_draft: false,
            allow_closed: false,
            delete_review_comments: false,
        }
    }
}

/// A enumeration of possible recommended actions for a Pull Request review.
#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub enum ReviewAction {
    /// Approve the current Pull Request's changes.
    Approve,

    /// Request changes to the current Pull Request's proposal.
    RequestChanges,

    /// Comment on the current Pull Request's changes without explicitly approving or requesting changes.
    #[default]
    Comment,
}

/// A struct to describe a single comment in a Pull Request review.
#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct ReviewComment {
    /// The file's line number in the diff that begins the the focus of the comment's concerns.
    pub line_start: Option<u32>,

    /// The file's line number in the diff that ends the focus of the comment's concerns.
    pub line_end: u32,

    /// The actual comment.
    ///
    /// This text can include a code block that demonstrates a suggested change(s).
    ///
    /// Typically, the comment should not begin with the [`ReviewOptions::marker`] value.
    /// That is managed by the git-bot-feedback library.
    pub comment: String,

    /// The file that this comment pertains to.
    pub path: String,
}
