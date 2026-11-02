//! Reviewing a project's branch before a merge: git on a machine with a
//! checkout reads the diff, read-only coding-agent sessions each review a
//! shard of it, and a fresh session verifies what they found.

pub(crate) mod git;
pub(crate) mod report;
mod work;

pub(crate) use work::{Fix, find_pr, start_fix};
