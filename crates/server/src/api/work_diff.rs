//! The diff viewer for a session's checkout or a project's branch: a desktop reads it now.

use super::{Reply, own_nodes::retired};
use crate::{
    api::router::Cx,
    protocol::{GetBranchDiff, GetWorkDiff, WorkDiff},
};

pub(super) async fn get_work_diff(_: Cx, _: GetWorkDiff) -> Reply<WorkDiff> {
    retired()
}

pub(super) async fn get_branch_diff(_: Cx, _: GetBranchDiff) -> Reply<WorkDiff> {
    retired()
}
