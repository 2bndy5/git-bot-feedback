use crate::CommentKind;

/// An enumeration of possible values that control [`FeedBackOptions::thread_comments`].
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

#[derive(Debug)]
pub struct ThreadCommentOptions {
    /// Controls posting comments on a thread that concerns a Pull Request or Push event.
    ///
    /// Typically, this is only desirable for Pull Requests.
    pub policy: CommentPolicy,

    /// The comment to post.
    ///
    /// This can be a blank string if [`Self::no_lgtm`] is true and the
    /// [`Self::kind`] is [`CommentKind::Lgtm`].
    pub comment: String,

    /// The [`CommentKind`] that describes the comment's purpose.
    pub kind: CommentKind,

    /// A string used to mark/identify the thread's comment as a comment submitted by this software.
    ///
    /// User comments may be indistinguishable from bot/generated comments if
    /// this value is not unique enough.
    ///
    /// If the git server employs Markdown syntax for comments, then
    /// it is recommended to set this to a HTML comment that is unique to
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

    /// Disallow posting "Looks Good To Me" comments.
    ///
    /// Setting this option to `true` may instigate the deletion of old bot comment(s),
    /// if any exist.
    pub no_lgtm: bool,
}

const DEFAULT_MARKER: &str = concat!(
    "<!-- ",
    env!("CARGO_CRATE_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    "/",
    env!("COMPILE_DATETIME"), // env var set by build.rs
    " -->\n"
);

impl Default for ThreadCommentOptions {
    fn default() -> Self {
        Self {
            policy: Default::default(),
            comment: Default::default(),
            kind: Default::default(),
            marker: DEFAULT_MARKER.to_string(),
            no_lgtm: Default::default(),
        }
    }
}

impl ThreadCommentOptions {
    pub(crate) fn mark_comment(&self) -> String {
        if !self.comment.starts_with(&self.marker) {
            return format!("{}{}", self.marker, self.comment);
        }
        self.comment.clone()
    }
}

#[cfg(test)]
mod test {
    use super::{DEFAULT_MARKER, ThreadCommentOptions};
    use chrono::NaiveDateTime;

    #[test]
    fn default_marker() {
        let mut opts = ThreadCommentOptions::default();
        assert_eq!(opts.marker, DEFAULT_MARKER);
        let datetime_start = concat!(
            "<!-- ",
            env!("CARGO_CRATE_NAME"),
            "/",
            env!("CARGO_PKG_VERSION"),
            "/",
        )
        .len();
        let datetime_end = DEFAULT_MARKER.len() - 5;
        let datetime_str = &DEFAULT_MARKER[datetime_start..datetime_end];
        NaiveDateTime::parse_from_str(datetime_str, "%b-%d-%Y_%H-%M").unwrap();
        assert_eq!(opts.mark_comment(), DEFAULT_MARKER);
        let comment = format!("{DEFAULT_MARKER}Some text data.");
        opts.comment = comment.clone();
        assert_eq!(opts.mark_comment(), comment);
    }
}
