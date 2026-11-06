//! Code reviews a node runs: a temporary reviewer session started on a
//! checkout's changes, by a coding agent's `review` tool or by a desktop,
//! and what the reviewer handed in with `submit_review`.

use super::Effort;
use serde::{Deserialize, Serialize};

/// What `SessionsRequest::StartReview` asks for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReviewOrder {
    /// The folder in the checkout to review.
    pub cwd: String,
    /// The reviewer's harness, such as `claude`.
    pub agent: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<Effort>,
    /// What the changes are measured against: `HEAD` for the uncommitted
    /// changes only, a ref or commit for everything since its merge base,
    /// empty for the checkout's default branch.
    #[serde(default)]
    pub base: String,
    /// What to look at most, in the user's words; empty for nothing in
    /// particular.
    #[serde(default)]
    pub focus: String,
    /// The session on the node whose work it reviews: the reviewer starts
    /// as its child. `None` for a review of a checkout alone.
    #[serde(default)]
    pub parent: Option<String>,
}

/// One review as the node keeps it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReviewRun {
    /// The reviewer session's id, which the review goes by after the
    /// session is gone.
    pub id: String,
    /// The session whose work it reviews, as the node names it.
    #[serde(default)]
    pub parent: Option<String>,
    pub cwd: String,
    pub base: String,
    /// The reviewer's harness.
    pub agent: String,
    /// Unix seconds.
    pub started_at: u64,
    /// What the reviewer submitted, once it did.
    #[serde(default)]
    pub review: Option<SubmittedReview>,
    /// Why it ended without a review.
    #[serde(default)]
    pub error: Option<String>,
}

impl ReviewRun {
    /// Still running: nothing submitted, nothing failed.
    pub fn running(&self) -> bool {
        self.review.is_none() && self.error.is_none()
    }
}

/// What a reviewer hands in with `submit_review`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubmittedReview {
    pub verdict: CodeVerdict,
    pub summary: String,
    #[serde(default)]
    pub findings: Vec<CodeFinding>,
}

/// The reviewer's word on the changes as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeVerdict {
    /// Good to merge.
    Approve,
    /// Something must change first.
    Changes,
    /// Remarks only.
    Comment,
}

/// One thing the reviewer found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeFinding {
    /// Relative to the checkout's top.
    pub file: String,
    /// The line in the new version of the file; `None` for the whole file.
    #[serde(default)]
    pub line: Option<u32>,
    pub severity: FindingSeverity,
    /// What is wrong, in a line.
    pub summary: String,
    /// How it fails: the input or sequence of events and what goes wrong.
    pub scenario: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    High,
    Medium,
    Low,
}
