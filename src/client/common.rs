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

/// Repository info for a PR branch.
#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Default)]
pub struct PrBranchRepo {
    /// The repository's ID.
    #[serde(default)]
    pub id: Option<u64>,
    /// The full repository name (eg. "owner/repo").
    #[serde(default)]
    pub full_name: Option<String>,
}

/// Branch info for a PR (head or base).
#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Default)]
pub struct PrBranchInfo {
    /// The repository info.
    #[serde(default)]
    pub repo: Option<PrBranchRepo>,
    /// The repository ID (sometimes directly on branch object in Gitea).
    #[serde(default)]
    pub repo_id: Option<u64>,
}

impl PrBranchInfo {
    /// Get the repository ID if available.
    pub fn repo_id(&self) -> Option<u64> {
        self.repo.as_ref().and_then(|r| r.id).or(self.repo_id)
    }

    /// Get the repository full name if available.
    pub fn repo_name(&self) -> Option<&str> {
        self.repo.as_ref().and_then(|r| r.full_name.as_deref())
    }
}

/// A structure for deserializing a Pull Request's info from a response's json.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct PullRequestInfo {
    /// Is this PR a draft?
    pub draft: bool,
    /// Is this PR locked?
    #[serde(alias = "is_locked")]
    pub locked: bool,
    /// The Pull Request's number.
    pub number: u64,
    /// What is current state of this PR?
    pub state: PullRequestState,
    /// Head branch info.
    #[serde(default)]
    pub head: Option<PrBranchInfo>,
    /// Base branch info.
    #[serde(default)]
    pub base: Option<PrBranchInfo>,
}

impl PullRequestInfo {
    /// Check if the PR originates from the same remote repository.
    pub fn is_same_remote(&self) -> bool {
        let (Some(head), Some(base)) = (&self.head, &self.base) else {
            return false;
        };
        if let (Some(head_id), Some(base_id)) = (head.repo_id(), base.repo_id()) {
            return head_id == base_id;
        }
        if let (Some(head_name), Some(base_name)) = (head.repo_name(), base.repo_name()) {
            return head_name == base_name;
        }
        false
    }
}
