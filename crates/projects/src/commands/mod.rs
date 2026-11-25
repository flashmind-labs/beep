//! Shell commands for a project's git and host work, and parsers for their output. Every
//! argument is quoted.

mod host;
mod parse;
pub use host::HostCommand;
pub(crate) use host::{GH_ISSUES, GH_OPEN_LIMIT, GH_PRS};
pub(crate) use parse::pages;
pub use parse::{
    ChecksState, OpenIssue, OpenPr, checks_of, json, parse_can_push, parse_login,
    parse_open_issues, parse_open_prs, parse_pr_head, text,
};

use crate::model::MachinePath;
use crate::{github_owner_var, quote};

/// Keeps `.worktrees/` and `.beep/` out of `git status` in the checkout.
pub fn exclude_project_dirs(checkout: &MachinePath) -> String {
    let checkout = quote(checkout.as_ref());
    format!(
        "exclude=\"$(git -C {checkout} rev-parse --path-format=absolute --git-common-dir)/info/exclude\" \
         && mkdir -p \"$(dirname \"$exclude\")\" \
         && for entry in .worktrees/ .beep/; do \
         grep -qxF \"$entry\" \"$exclude\" 2>/dev/null || echo \"$entry\" >> \"$exclude\"; done"
    )
}

/// Prefix that makes `gh` run as the user's account for `repo`'s owner (see
/// `credentials::for_machines`). Only the variable's name is in the command, never the token.
pub fn gh_account(repo: &str) -> String {
    repo.split_once('/')
        .and_then(|(owner, _)| github_owner_var(owner))
        .map(|var| format!("GH_TOKEN=\"${{{var}:-$GH_TOKEN}}\" "))
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "../tests/commands.rs"]
mod tests;
