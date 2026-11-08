//! Pulling and pushing the branch a checkout has out, and reading how far it is from its
//! upstream. The scripts run on the machine that holds the checkout, with its git sign-in.

use crate::{NO_GIT_SH, Shell, branches::tail, quote};
use anyhow::{Result, bail};
use serde::Deserialize;
use std::time::Duration;

/// How long reading a checkout's state may take, the fetch included.
pub const STATE_LIMIT: Duration = Duration::from_secs(40);
/// How long a pull or a push may take.
pub const MOVE_LIMIT: Duration = Duration::from_secs(120);

/// Which way a branch moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Move {
    Pull,
    Push,
}

/// The branch a checkout has out and how far it is from its upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutState {
    /// `None` on a detached HEAD.
    pub branch: Option<String>,
    /// `None` before the first push.
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

/// Commands that never wait for a person to type a password.
const NO_PROMPT: &str = "export GIT_TERMINAL_PROMPT=0 GCM_INTERACTIVE=never; ";

/// Prints the branch, its upstream and the distance to it. The fetch only moves remote refs and
/// its failure (offline, no sign-in) leaves the last known counts. Exits 3 with no folder.
pub fn state_script(folder: &str) -> String {
    state_read(folder, true)
}

/// Reads the current local refs without fetching or writing files watched by subscriptions.
pub fn local_state_script(folder: &str) -> String {
    state_read(folder, false)
}

fn state_read(folder: &str, fetch: bool) -> String {
    format!(
        "cd -- {folder} || exit 3; {NO_PROMPT}\
         b=$(git symbolic-ref -q --short HEAD) || {{ echo '@@detached'; exit 0; }}; \
         echo \"@@branch $b\"; \
         {fetch}\
         if u=$(git rev-parse --abbrev-ref '@{{upstream}}' 2>/dev/null); then \
           echo \"@@upstream $u\"; \
           echo \"@@ab $(git rev-list --left-right --count 'HEAD...@{{upstream}}' 2>/dev/null)\"; \
         fi; exit 0",
        folder = quote(folder),
        fetch = if fetch {
            "git fetch --quiet --no-write-fetch-head >/dev/null 2>&1; "
        } else {
            ""
        },
    )
}

/// Parses [`state_script`].
pub fn parse_state(output: &str) -> Result<CheckoutState> {
    let mut state = CheckoutState {
        branch: None,
        upstream: None,
        ahead: 0,
        behind: 0,
    };
    let mut seen = false;
    for line in output.lines() {
        if line.starts_with("@@detached") {
            seen = true;
        } else if let Some(name) = line.strip_prefix("@@branch ") {
            state.branch = Some(name.trim().to_owned());
            seen = true;
        } else if let Some(name) = line.strip_prefix("@@upstream ") {
            state.upstream = Some(name.trim().to_owned());
        } else if let Some(counts) = line.strip_prefix("@@ab ") {
            let mut parts = counts.split_whitespace();
            if let (Some(ahead), Some(behind)) = (parts.next(), parts.next()) {
                state.ahead = ahead.parse().unwrap_or(0);
                state.behind = behind.parse().unwrap_or(0);
            }
        }
    }
    if !seen {
        bail!("git did not answer as expected");
    }
    Ok(state)
}

/// Pulls fast-forward only, or pushes, `branch` of the checkout in `folder`. Never merges,
/// rebases or forces; a push with no upstream sets it. Exits 3 no folder, 8 pull with no
/// upstream, 9 detached HEAD, 10 another branch is out now; otherwise git's own code.
pub fn move_script(folder: &str, branch: &str, which: Move) -> String {
    let act = match which {
        Move::Pull => {
            "git rev-parse -q --verify '@{upstream}' >/dev/null 2>&1 || exit 8; \
             git fetch --no-write-fetch-head 2>&1 || exit $?; \
             cur=$(git symbolic-ref -q --short HEAD) || exit 9; \
             [ \"$cur\" = \"$b\" ] || exit 10; \
             git -c merge.autostash=false merge --ff-only --no-edit '@{upstream}' 2>&1"
        }
        // The full ref name keeps a branch that starts with `-` from reading as an option.
        Move::Push => {
            "if git rev-parse -q --verify '@{upstream}' >/dev/null 2>&1; then git push 2>&1; \
             else git push -u origin \"refs/heads/$b\" 2>&1; fi"
        }
    };
    format!(
        "cd -- {folder} || exit 3; {NO_PROMPT}b={branch}; \
         cur=$(git symbolic-ref -q --short HEAD) || exit 9; \
         [ \"$cur\" = \"$b\" ] || exit 10; {act}",
        folder = quote(folder),
        branch = quote(branch),
    )
}

/// Why a pull or push did not happen, in words for the user.
pub fn move_failure(code: Option<i32>, output: &str) -> String {
    match code {
        None => "git did not finish in time.".to_owned(),
        Some(3) => "The project's folder is not there.".to_owned(),
        Some(8) => "The branch has no upstream to pull from.".to_owned(),
        Some(9) => "The checkout is not on a branch.".to_owned(),
        Some(10) => "The checkout is on another branch now.".to_owned(),
        Some(_) if signed_out(output) => {
            "Not signed in to git on this machine. Check Git sign-ins in Settings.".to_owned()
        }
        Some(_) => tail(output).to_owned(),
    }
}

fn signed_out(output: &str) -> bool {
    let lower = output.to_lowercase();
    [
        "authentication failed",
        "could not read username",
        "could not read password",
        "terminal prompts disabled",
        "permission denied",
        "invalid username or password",
        "returned error: 403",
        "returned error: 401",
    ]
    .iter()
    .any(|sign| lower.contains(sign))
}

/// Reads the state of the checkout in `folder`.
pub async fn read_state(shell: &dyn Shell, folder: &str) -> Result<CheckoutState> {
    let ran = shell.run(&state_script(folder), STATE_LIMIT).await?;
    match ran.exit_code {
        Some(0) => parse_state(&ran.output),
        Some(3) => bail!("the project's folder is not there"),
        Some(NO_GIT_SH) => bail!("{}", tail(&ran.output)),
        Some(code) => bail!("exited with {code}: {}", tail(&ran.output)),
        None => bail!("did not finish in time"),
    }
}

/// Pulls or pushes `branch` of the checkout in `folder`; the error is for the user.
pub async fn run_move(
    shell: &dyn Shell,
    folder: &str,
    branch: &str,
    which: Move,
) -> Result<(), String> {
    let ran = shell
        .run(&move_script(folder, branch, which), MOVE_LIMIT)
        .await
        .map_err(|e| format!("{e:#}"))?;
    match ran.exit_code {
        Some(0) => Ok(()),
        code => Err(move_failure(code, &ran.output)),
    }
}

#[cfg(test)]
#[path = "tests/checkout_sync.rs"]
mod tests;
