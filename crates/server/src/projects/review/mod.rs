//! Reviewing a project's branch before a merge: git on a machine with a
//! checkout reads the diff, read-only coding-agent sessions each review a
//! shard of it in a detached worktree, and a verifier on the cheapest
//! offered model checks each finding. [`drive`] moves reviews on in the
//! background.

pub(crate) mod drive;
pub(crate) mod git;
pub(crate) mod report;
pub(crate) mod rules;
pub(crate) mod shard;
mod work;

pub(crate) use work::{Fix, find_pr, start_fix};
