//! What changed in a checkout, for the apps' diff viewer (`read`): the git
//! diff from where the head left the base, plus the working tree's changes and
//! untracked files when the head is the working tree.

mod parse;
#[cfg(feature = "host")]
mod read;

use beep_proto::{
    MAX_DIFF_FILE_LINES, MAX_DIFF_LINES, MAX_DIFF_SHOWN_LINES, WorkDiff, node::agent::DiffAsk,
};
use parse::{Caps, DiffParser, Keep};

#[cfg(feature = "host")]
pub(super) use read::{git, read};

/// Bytes of text one file keeps, and all of them.
const FILE_BYTES: usize = 512 * 1024;
const TOTAL_BYTES: usize = 4 * 1024 * 1024;
/// Bytes of text a file named in `only` keeps.
const SHOWN_BYTES: usize = 4 * 1024 * 1024;

fn patch_caps(shown: bool) -> Caps {
    if shown {
        Caps {
            file_lines: MAX_DIFF_SHOWN_LINES,
            file_bytes: SHOWN_BYTES,
            total_lines: MAX_DIFF_SHOWN_LINES,
            total_bytes: SHOWN_BYTES,
        }
    } else {
        Caps {
            file_lines: MAX_DIFF_FILE_LINES,
            file_bytes: FILE_BYTES,
            total_lines: MAX_DIFF_LINES,
            total_bytes: TOTAL_BYTES,
        }
    }
}

/// A parser for the files `ask` wants, and the limits it keeps to.
fn parser(ask: &DiffAsk) -> (DiffParser, Caps) {
    let shown = !ask.only.is_empty();
    let caps = patch_caps(shown);
    let keep = Keep {
        only: ask.only.iter().cloned().collect(),
        lockfiles: ask.lockfiles || shown,
    };
    (DiffParser::new(caps, keep), caps)
}

/// Parses a complete Git patch with the same limits as a live worktree diff.
pub fn parse_git_patch(ask: &DiffAsk, text: &str) -> WorkDiff {
    let (mut parser, _) = parser(ask);

    for line in text.lines() {
        parser.line(line);
    }

    let parsed = parser.finish();
    WorkDiff {
        root: ask.root.clone(),
        base: ask.base.clone(),
        merge_base: ask.base.clone(),
        working_tree: false,
        files: parsed.files,
        additions: parsed.additions,
        deletions: parsed.deletions,
        more: parsed.more,
    }
}
