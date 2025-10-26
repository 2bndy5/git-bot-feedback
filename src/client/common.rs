#![cfg(any(feature = "gitea", feature = "github"))]

use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum PullRequestState {
    Open,
    Closed,
}

/// PR event payload.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct PullRequestEventPayload {
    /// The Pull Request's info.
    pub pull_request: PullRequestInfo,
}

/// A structure for deserializing a Pull Request's info from a response's json.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct PullRequestInfo {
    /// Is this PR a draft?
    pub draft: bool,
    /// Is this PR locked?
    pub locked: bool,
    /// The Pull Request's number.
    pub number: u64,
    /// What is current state of this PR?
    pub state: PullRequestState,
}
