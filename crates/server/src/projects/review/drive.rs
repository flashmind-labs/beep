//! Where a review stands. Reviewers ran on the user's machines, which the server no longer
//! reaches, so no review moves on here.

use crate::store::{ShardRow, ShardState};

/// Whether any shard still reads or waits for its verifier.
pub(crate) fn running(shards: &[ShardRow]) -> bool {
    shards
        .iter()
        .any(|shard| matches!(shard.state, ShardState::Reading | ShardState::Verifying))
}
