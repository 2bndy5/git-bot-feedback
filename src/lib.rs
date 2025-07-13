#![doc = include_str!("../README.md")]
pub mod client;
pub mod error;

use std::fmt::Display;

pub use client::{RestApiClient, RestApiRateLimitHeaders};
pub use error::RestClientError;
mod thread_comments;
pub use thread_comments::{CommentPolicy, ThreadCommentOptions};

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

/// A type to represent an output variable.
///
/// This is akin to the key/value pairs used in most
/// config file formats but with some limitations:
///
/// - Both [OutputVariable::name] and [OutputVariable::value] must be UTF-8 encoded.
/// - The [OutputVariable::value] cannot span multiple lines.
#[derive(Debug, Clone)]
pub struct OutputVariable {
    /// The output variable's name.
    pub name: String,

    /// The output variable's value.
    pub value: String,
}

impl OutputVariable {
    pub(crate) fn validate(&self) -> bool {
        !self.value.contains("\n")
    }
}

impl Display for OutputVariable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} = {}", self.name, self.value)
    }
}
