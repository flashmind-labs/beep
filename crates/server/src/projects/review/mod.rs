//! Branch review before a merge: sharded read-only reviewers, then a verifier per finding.

pub(crate) mod drive;
pub(crate) mod rules;
mod work;

pub(crate) use work::{Fix, start_fix};
