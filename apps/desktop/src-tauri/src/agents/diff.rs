//! The diff viewer: what changed in a coding agent session's checkout, read
//! by the node where the checkout is.

use crate::{
    beep::Service,
    error::Result,
    fleet::{Machines, agents as agent_nodes, files as direct},
};
use beep_proto::{
    DiffFile, DiffHunk, DiffLeftOut, DiffLine, DiffLineKind, DiffStatus, GetWorkDiff, WorkDiff,
    node::agent::{DiffAsk, Worktree},
};
use serde::Serialize;

/// A diff for the web view (DiffView).
#[derive(Debug, Serialize)]
pub struct DiffView {
    root: String,
    base: String,
    merge_base: String,
    working_tree: bool,
    files: Vec<DiffFileView>,
    additions: u32,
    deletions: u32,
    more: u32,
}

#[derive(Debug, Serialize)]
pub struct DiffFileView {
    path: String,
    old_path: Option<String>,
    /// `added`, `modified`, `deleted`, `renamed` or `binary`.
    status: &'static str,
    additions: u32,
    deletions: u32,
    hunks: Vec<DiffHunkView>,
    /// `too_large`, `lockfile` or `over_total`.
    left_out: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct DiffHunkView {
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
    section: String,
    lines: Vec<DiffLineView>,
}

/// One line, short-named as there are many: `k` is `+`, `-` or ` `, `t`
/// the text and `e` set when the file ends after it without a newline.
#[derive(Debug, Serialize)]
pub struct DiffLineView {
    k: &'static str,
    t: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    e: bool,
}

fn status_name(status: DiffStatus) -> &'static str {
    match status {
        DiffStatus::Added {} => "added",
        DiffStatus::Modified {} => "modified",
        DiffStatus::Deleted {} => "deleted",
        DiffStatus::Renamed {} => "renamed",
        DiffStatus::Binary {} => "binary",
    }
}

fn left_out_name(left: DiffLeftOut) -> &'static str {
    match left {
        DiffLeftOut::TooLarge {} => "too_large",
        DiffLeftOut::Lockfile {} => "lockfile",
        DiffLeftOut::OverTotal {} => "over_total",
    }
}

fn line_view(line: DiffLine) -> DiffLineView {
    let k = match line.kind {
        DiffLineKind::Added {} => "+",
        DiffLineKind::Removed {} => "-",
        DiffLineKind::Context {} => " ",
    };
    DiffLineView {
        k,
        t: line.text,
        e: line.no_eol,
    }
}

fn hunk_view(hunk: DiffHunk) -> DiffHunkView {
    DiffHunkView {
        old_start: hunk.old_start,
        old_lines: hunk.old_lines,
        new_start: hunk.new_start,
        new_lines: hunk.new_lines,
        section: hunk.section,
        lines: hunk.lines.into_iter().map(line_view).collect(),
    }
}

fn file_view(file: DiffFile) -> DiffFileView {
    DiffFileView {
        path: file.path,
        old_path: file.old_path,
        status: status_name(file.status),
        additions: file.additions,
        deletions: file.deletions,
        hunks: file.hunks.into_iter().map(hunk_view).collect(),
        left_out: file.left_out.map(left_out_name),
    }
}

fn view(diff: WorkDiff) -> DiffView {
    DiffView {
        root: diff.root,
        base: diff.base,
        merge_base: diff.merge_base,
        working_tree: diff.working_tree,
        files: diff.files.into_iter().map(file_view).collect(),
        additions: diff.additions,
        deletions: diff.deletions,
        more: diff.more,
    }
}

/// What changed in the session folder `root` on `machine_id`, against its
/// default branch, uncommitted and untracked changes in. `only` asks for
/// those files in full, past the caps.
#[tauri::command]
pub async fn work_diff(
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
    root: String,
    only: Vec<String>,
    lockfiles: bool,
) -> Result<DiffView> {
    let diff = match fleet.member(&machine_id) {
        Some(machine) => {
            let ask = DiffAsk {
                root,
                base: String::new(),
                head: String::new(),
                only,
                lockfiles,
            };
            direct::work_diff(&fleet, machine, ask).await?
        }
        None => {
            beep.call(GetWorkDiff {
                machine_id,
                root,
                base: String::new(),
                head: String::new(),
                only,
                lockfiles,
            })
            .await?
        }
    };
    Ok(view(diff))
}

/// The worktrees of the repository holding the session folder `folder` on
/// `machine_id`. Empty for a machine only a server reaches.
#[tauri::command]
pub async fn agent_worktrees(
    fleet: Machines<'_>,
    machine_id: String,
    folder: String,
) -> Result<Vec<Worktree>> {
    match fleet.member(&machine_id) {
        Some(machine) => agent_nodes::worktrees(&fleet, machine, &folder).await,
        None => Ok(Vec::new()),
    }
}
