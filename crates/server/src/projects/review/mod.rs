//! Branch review before a merge: sharded read-only reviewers, then a verifier per finding.

pub(crate) mod drive;
pub(crate) mod git;
pub(crate) mod report;
pub(crate) mod rules;
pub(crate) mod shard;
mod work;

pub(crate) use work::{Fix, find_pr, start_fix};
