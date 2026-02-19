pub mod review_comments;
pub mod thread_comments;

/// An enumeration of possible type of comments being posted.
///
/// The default is [`CommentKind::Concerns`].
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum CommentKind {
    /// A comment that admonishes concerns for end-users' attention.
    #[default]
    Concerns,

    /// A comment that basically says "Looks Good To Me".
    Lgtm,
}

/// An enumeration of supported behaviors about posting comments.
///
/// See [`ThreadCommentOptions::policy`].
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum CommentPolicy {
    /// Each thread comment is posted as a new comment.
    ///
    /// This may result in perceivable spam because
    /// every new comment may cause notification emails.
    Anew,

    /// Like [`CommentPolicy::Anew`], but updates a single comment.
    ///
    /// Typically, this is the desirable option when posting thread comments.
    #[default]
    Update,
}

pub const DEFAULT_MARKER: &str = concat!(
    "<!-- ",
    env!("CARGO_CRATE_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    "/",
    env!("COMPILE_DATETIME"), // env var set by build.rs
    " -->\n"
);
