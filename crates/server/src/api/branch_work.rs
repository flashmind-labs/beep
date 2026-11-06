//! Giving a project's branch a worktree on one of the caller's machines: a desktop does it now.

use super::{Reply, own_nodes::retired};
use crate::{
    api::router::Cx,
    protocol::{AddBranchWorktree, ProjectBranch},
};

pub(super) async fn add_branch_worktree(_: Cx, _: AddBranchWorktree) -> Reply<ProjectBranch> {
    retired()
}
