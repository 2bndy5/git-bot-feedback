#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod client;
pub use client::{RestApiClient, RestApiRateLimitHeaders};
pub mod error;
pub use error::RestClientError;
mod comments;
pub use comments::{
    review_comments::{ReviewAction, ReviewComment, ReviewOptions},
    thread_comments::{CommentKind, CommentPolicy, ThreadCommentOptions},
};
mod output_variable;
pub use output_variable::OutputVariable;
mod file_annotations;
pub use file_annotations::{AnnotationLevel, FileAnnotation};

#[cfg(feature = "file-changes")]
mod git_diff;
#[cfg(feature = "file-changes")]
pub use git_diff::{DiffHunkHeader, parse_diff};
#[cfg(feature = "file-changes")]
mod file_utils;
#[cfg(feature = "file-changes")]
pub use file_utils::{FileDiffLines, LinesChangedOnly, file_filter::FileFilter};

// Re-export dependencies for users of optional feature
#[cfg(feature = "file-changes")]
#[cfg_attr(docsrs, doc(cfg(feature = "file-changes")))]
pub use fast_glob;
#[cfg(feature = "file-changes")]
#[cfg_attr(docsrs, doc(cfg(feature = "file-changes")))]
pub use regex;
