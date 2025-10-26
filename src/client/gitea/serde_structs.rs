//! This submodule declares data structures used to
//! deserialize (and serializer) JSON payload data.

use serde::Deserialize;

// /// A structure for deserializing a Pull Request's info from a response's json.
// #[derive(Debug, Deserialize, PartialEq, Clone)]
// pub struct PullRequestInfo {
//     /// Is this PR a draft?
//     pub draft: bool,
//     /// What is current state of this PR?
//     ///
//     /// Here we only care if it is `"open"`.
//     pub state: String,
// }

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
