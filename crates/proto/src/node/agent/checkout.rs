//! The branch or worktree a session starts on, and how they are named.

use serde::{Deserialize, Serialize};

/// A new branch a session starts on, made from `base` (a branch, tag or
/// commit; the checkout's `HEAD` when `None`). Its name comes from the
/// prompt, as `prompt_branch` makes it, with `-2`, `-3` added while that
/// is taken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CheckoutPlan {
    /// Switches the checkout itself to the branch; refused when it has
    /// uncommitted changes.
    NewBranch { base: Option<String> },
    /// Adds a worktree on the branch in the checkout, where
    /// `worktree_path` puts it, and starts there.
    NewWorktree { base: Option<String> },
    /// `NewWorktree` on a branch named `branch` (with `-2`, `-3` added
    /// while that is taken) rather than from the prompt. A node from before
    /// it does not read the start.
    NamedWorktree {
        base: Option<String>,
        branch: String,
    },
    /// Starts on the existing branch `branch`: in the worktree that has it
    /// checked out, else in a new one where `worktree_path` puts it, made
    /// after fetching `branch` from `origin`. Like `NamedWorktree`, a
    /// node from before it does not read the start.
    BranchWorktree { branch: String },
    /// A throwaway copy of the work in the checkout: a worktree on its
    /// `HEAD`, detached, with its uncommitted changes and untracked files
    /// copied in. The node removes it when the session is closed or
    /// deleted. Like `NamedWorktree`, a node from before it does not read
    /// the start.
    CopyWorktree,
}

impl CheckoutPlan {
    /// The ref the branch is made from, when one is named.
    pub fn base(&self) -> Option<&str> {
        match self {
            Self::NewBranch { base }
            | Self::NewWorktree { base }
            | Self::NamedWorktree { base, .. } => base.as_deref(),
            Self::BranchWorktree { .. } | Self::CopyWorktree => None,
        }
    }

    /// The branch it names, for the plans that name one.
    pub fn branch(&self) -> Option<&str> {
        match self {
            Self::NamedWorktree { branch, .. } | Self::BranchWorktree { branch } => Some(branch),
            Self::NewBranch { .. } | Self::NewWorktree { .. } | Self::CopyWorktree => None,
        }
    }
}

/// Whether `branch` may name a branch a plan makes or starts on: a
/// `valid_ref` that is not `HEAD`, has no `~` or `^`, and no part that is
/// empty, starts with `.` or ends in `.lock`. The node checks it again.
pub fn valid_branch(branch: &str) -> bool {
    valid_ref(branch)
        && branch != "HEAD"
        && !branch.contains(['~', '^'])
        && branch
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}

/// The longest base ref a checkout plan may name.
pub const MAX_REF: usize = 200;

/// Whether `base` may name the ref a new branch starts from: short, not an
/// option, and nothing git refuses in a ref anywhere (spaces, control
/// characters, `..`, `:`). `~` and `^` stay, for `HEAD~2`.
pub fn valid_ref(base: &str) -> bool {
    !base.is_empty()
        && base.len() <= MAX_REF
        && !base.starts_with('-')
        && !base.contains("..")
        && !base.contains("@{")
        && !base.chars().any(|c| {
            c.is_control() || c.is_whitespace() || matches!(c, ':' | '?' | '*' | '[' | '\\')
        })
}

/// The longest slug `slug` makes.
pub(crate) const MAX_SLUG: usize = 40;

/// A short name for `text`, for a branch or folder: its ASCII letters and
/// digits in lowercase, each run of anything else as one `-`, at most
/// `MAX_SLUG` long, with no `-` at either end. Empty when `text` has no
/// letter or digit. Project tasks name their branches the same way.
pub fn slug(text: &str) -> String {
    let mut slug = String::new();

    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    // Only ASCII is pushed, so any byte index is a char boundary.
    slug.truncate(MAX_SLUG);
    slug.trim_matches('-').to_owned()
}

/// The branch a session started on `prompt` gets: `beep/<slug>`, or
/// `beep/agent` for a prompt without a letter or digit.
pub fn prompt_branch(prompt: &str) -> String {
    let slug = slug(prompt);
    let slug = if slug.is_empty() { "agent" } else { &slug };
    format!("beep/{slug}")
}

/// The folder name a throwaway copy's worktree starts with, in the
/// checkout's `.worktrees/`.
pub const COPY_PREFIX: &str = "beep-copy-";

/// Where `branch`'s worktree goes in its checkout: `.worktrees/` and the
/// name with each `/` as `-`, as the desktop's `worktreeName` and the
/// Branches tile name it.
pub fn worktree_path(branch: &str) -> String {
    format!(".worktrees/{}", branch.replace('/', "-"))
}
