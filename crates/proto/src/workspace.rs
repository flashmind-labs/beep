//! Folders, trees and diffs of a project's working copy.

use serde::{Deserialize, Serialize};

/// The largest file `WriteWorkFile` writes.
pub const MAX_WORK_UPLOAD: u64 = 64 * 1024 * 1024;

/// The most entries one `ListWorkFolder` gives.
pub const MAX_WORK_ENTRIES: usize = 2000;

/// The most levels one `ListWorkTree` reads.
pub const MAX_TREE_DEPTH: u32 = 6;

/// About the most entries one `ListWorkTree` gives: it stops reading
/// folders past it.
pub const MAX_TREE_ENTRIES: usize = 5000;

/// Folders of a session's working folder, as `ListWorkTree` reads them.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkTree {
    /// The folder asked first, then those under it, each in full. A folder
    /// not here is not read yet.
    pub folders: Vec<WorkFolder>,
    /// It stopped at [`MAX_TREE_ENTRIES`] before `depth`.
    pub truncated: bool,
}

/// One folder of a session's working folder, as `ListWorkFolder` reads it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkFolder {
    /// The folder, relative to the root, `""` for the root.
    pub path: String,
    /// Folders first, then files, each by name.
    pub entries: Vec<WorkEntry>,
    /// Entries left out past [`MAX_WORK_ENTRIES`].
    pub more: u32,
    /// The folder is in a git checkout, so `ignored` and `change` mean
    /// something.
    pub git: bool,
}

/// A file or folder in a `WorkFolder`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkEntry {
    pub name: String,
    pub folder: bool,
    /// A file's size in bytes; 0 for a folder.
    pub size: u64,
    /// Git ignores it.
    pub ignored: bool,
    /// What git sees changed in it, or anywhere under a folder.
    pub change: Option<GitChange>,
}

wire_enum! {
    /// How git sees a file changed, as `WorkEntry::change` marks it. A
    /// folder takes the strongest of what is under it, the last here first.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum GitChange {
        0 Untracked {}
        1 Added {}
        2 Modified {}
        3 Conflicted {}
    }
}

/// The most changed lines one file of a `WorkDiff` gives.
pub const MAX_DIFF_FILE_LINES: u32 = 2_000;

/// The most lines a `WorkDiff` gives in all.
pub const MAX_DIFF_LINES: u32 = 20_000;

/// The most lines a file named in `GetWorkDiff::only` gives.
pub const MAX_DIFF_SHOWN_LINES: u32 = 50_000;

/// The most files a `WorkDiff` lists; `WorkDiff::more` counts the rest.
pub const MAX_DIFF_FILES: usize = 1_000;

/// What changed in a checkout, as `GetWorkDiff` reads it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkDiff {
    /// The folder the diff was read in, absolute: a branch's worktree
    /// when it has one. File paths are relative to it.
    pub root: String,
    /// What it compares against, as git names it (`origin/main`), or
    /// `HEAD` in a checkout without a default branch.
    pub base: String,
    /// The commit the changes start from, where the head left the base;
    /// empty before the first commit.
    pub merge_base: String,
    /// Uncommitted changes and untracked files are in it.
    pub working_tree: bool,
    pub files: Vec<DiffFile>,
    /// Added and removed lines in every file, those without lines too.
    pub additions: u32,
    pub deletions: u32,
    /// Files left out past [`MAX_DIFF_FILES`].
    pub more: u32,
}

/// One file of a `WorkDiff`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffFile {
    /// Relative to `WorkDiff::root`, `/` between names; the new path of a
    /// rename.
    pub path: String,
    /// Where a renamed file was.
    pub old_path: Option<String>,
    pub status: DiffStatus,
    pub additions: u32,
    pub deletions: u32,
    /// Empty for a binary file, and when `left_out` says why.
    pub hunks: Vec<DiffHunk>,
    pub left_out: Option<DiffLeftOut>,
}

/// One `@@` hunk of a `DiffFile`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// What git shows after the second `@@`, such as the function.
    pub section: String,
    pub lines: Vec<DiffLine>,
}

/// One line of a hunk, without its marker or line ending.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
    /// The file ends after this line without a newline.
    pub no_eol: bool,
}

wire_enum! {
    /// How a `DiffFile` changed.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum DiffStatus {
        0 Added {}
        1 Modified {}
        2 Deleted {}
        3 Renamed {}
        4 Binary {}
    }
}

wire_enum! {
    /// What a `DiffLine` is: unchanged context, added or removed.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum DiffLineKind {
        0 Context {}
        1 Added {}
        2 Removed {}
    }
}

wire_enum! {
    /// Why a `DiffFile` comes without its lines: past
    /// [`MAX_DIFF_FILE_LINES`], a lockfile asked without `lockfiles`, or
    /// past [`MAX_DIFF_LINES`] with the files before it.
    /// `GetWorkDiff::only` asks for them.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum DiffLeftOut {
        0 TooLarge {}
        1 Lockfile {}
        2 OverTotal {}
    }
}
