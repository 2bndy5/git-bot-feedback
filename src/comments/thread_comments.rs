#[cfg(feature = "pyo3")]
use pyo3::prelude::*;

use super::DEFAULT_MARKER;

/// An enumeration of possible type of comments being posted.
///
/// The default is [`CommentKind::Concerns`].
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "pyo3", pyclass(module = "git_bot_feedback", from_py_object))]
pub enum CommentKind {
    /// A comment that admonishes concerns for end-users' attention.
    #[default]
    Concerns,

    /// A comment that basically says "Looks Good To Me".
    Lgtm,
}

/// An enumeration of supported behaviors about posting comments.
///
/// See [`ThreadCommentOptions::policy`](crate::ThreadCommentOptions::policy).
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "pyo3", pyclass(module = "git_bot_feedback", from_py_object))]
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

/// Options that control posting comments on a thread.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "pyo3",
    pyclass(module = "git_bot_feedback", from_py_object, get_all, set_all)
)]
pub struct ThreadCommentOptions {
    /// Controls posting comments on a thread that concerns a Pull Request or Push event.
    ///
    /// Typically, this is only desirable for Pull Requests.
    pub policy: CommentPolicy,

    /// The comment to post.
    pub comment: String,

    /// The [`CommentKind`] that describes the comment's purpose.
    pub kind: CommentKind,

    /// A string used to mark/identify the thread's comment as a comment submitted by this software.
    ///
    /// User comments may be indistinguishable from bot/generated comments if
    /// this value is not unique enough.
    ///
    /// If the git server employs Markdown syntax for comments, then
    /// it is recommended to set this to an HTML comment that is unique to
    /// your CI application:
    ///
    /// ``<!-- my-cool-CI-app-name -->``
    ///
    /// The default value for this is an HTML comment generated from
    /// this crate's name and version along with the compile-tome's datetime.
    /// For example:
    ///
    /// ``<!-- git-bot-feedback/0.1.0/Jul-14-2025_17-00 -->``
    pub marker: String,

    /// Disallow posting "Looks Good To Me" comments.
    ///
    /// Setting this option to `true` may instigate the deletion of old bot comment(s),
    /// if any exist.
    pub no_lgtm: bool,
}

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
    /// Ensure that the [`ThreadCommentOptions::comment`] is marked with
    /// the [`ThreadCommentOptions::marker`].
    ///
    /// Typically only used by implementations of
    /// [`RestApiClient::post_thread_comment`](crate::client::RestApiClient::post_thread_comment)
    /// and [`RestApiClient::append_step_summary`](crate::client::RestApiClient::append_step_summary).
    pub fn mark_comment(&self) -> String {
        if !self.comment.starts_with(&self.marker) {
            return format!("{}{}", self.marker, self.comment);
        }
        self.comment.clone()
    }
}

#[cfg(feature = "pyo3")]
#[pymethods]
impl ThreadCommentOptions {
    /// Create a new instance of ``ThreadCommentOptions``.
    #[new]
    #[pyo3(
        signature = (
            policy = None,
            comment = None,
            kind = None,
            marker = None,
            no_lgtm = None,
        ),
        text_signature = "(policy: CommentPolicy | None = None, comment: str | None = None, kind: CommentKind | None = None, marker: str | None = None, no_lgtm: bool = False)",
    )]
    pub fn new(
        policy: Option<CommentPolicy>,
        comment: Option<String>,
        kind: Option<CommentKind>,
        marker: Option<String>,
        no_lgtm: Option<bool>,
    ) -> Self {
        Self {
            policy: policy.unwrap_or_default(),
            comment: comment.unwrap_or_default(),
            kind: kind.unwrap_or_default(),
            marker: marker.unwrap_or_else(|| DEFAULT_MARKER.to_string()),
            no_lgtm: no_lgtm.unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod test {
    #![allow(clippy::unwrap_used)]

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
