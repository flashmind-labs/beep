//! Pulling and pushing the branch a checkout has out, and reading how far it is from its
//! upstream. The scripts run on the machine that holds the checkout, with its git sign-in.

use crate::{Shell, branches::tail, quote};
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
    format!(
        "cd -- {folder} || exit 3; {NO_PROMPT}\
         b=$(git symbolic-ref -q --short HEAD) || {{ echo '@@detached'; exit 0; }}; \
         echo \"@@branch $b\"; \
         git fetch --quiet >/dev/null 2>&1; \
         if u=$(git rev-parse --abbrev-ref '@{{upstream}}' 2>/dev/null); then \
           echo \"@@upstream $u\"; \
           echo \"@@ab $(git rev-list --left-right --count 'HEAD...@{{upstream}}' 2>/dev/null)\"; \
         fi; exit 0",
        folder = quote(folder),
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
             git pull --ff-only 2>&1"
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
mod tests {
    use super::*;
    use std::{fs, path::Path, process::Command};
    use tempfile::TempDir;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn git_out(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git runs");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Runs the script as a node's shell would: its code and what it printed.
    fn sh(script: &str) -> (Option<i32>, String) {
        let out = Command::new("sh")
            .arg("-c")
            .arg(script)
            .output()
            .expect("sh runs");
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    }

    fn path(folder: &Path) -> &str {
        folder.to_str().expect("a UTF-8 path")
    }

    fn state(folder: &Path) -> CheckoutState {
        let (code, out) = sh(&state_script(path(folder)));
        assert_eq!(code, Some(0));
        parse_state(&out).expect("state parses")
    }

    fn run(folder: &Path, branch: &str, which: Move) -> Option<i32> {
        sh(&move_script(path(folder), branch, which)).0
    }

    fn commit(dir: &Path, file: &str) {
        fs::write(dir.join(file), file).expect("a file");
        git(dir, &["add", file]);
        git(dir, &["commit", "-m", file]);
    }

    #[test]
    fn pull_and_push_move_only_when_git_can_do_it_cleanly() {
        let tmp = TempDir::new().expect("a temp dir");
        let root = tmp.path();
        git(root, &["init", "--bare", "-b", "main", "origin.git"]);
        let (one, two) = (root.join("one"), root.join("two"));
        git(root, &["clone", "origin.git", "one"]);
        git(&one, &["checkout", "-b", "main"]);
        commit(&one, "a.txt");
        git(&one, &["push", "-u", "origin", "main"]);
        git(root, &["clone", "origin.git", "two"]);

        let fresh = state(&two);
        assert_eq!(fresh.branch.as_deref(), Some("main"));
        assert_eq!(fresh.upstream.as_deref(), Some("origin/main"));
        assert_eq!((fresh.ahead, fresh.behind), (0, 0));

        // A fast-forward pull; the state sees the other clone's commit through its fetch.
        commit(&one, "b.txt");
        git(&one, &["push"]);
        assert_eq!(state(&two).behind, 1);
        assert_eq!(run(&two, "main", Move::Pull), Some(0));
        assert!(two.join("b.txt").exists());
        assert_eq!(state(&two).behind, 0);

        // Diverged: the pull is refused and nothing changes.
        commit(&one, "c.txt");
        git(&one, &["push"]);
        commit(&two, "d.txt");
        let head = git_out(&two, &["rev-parse", "HEAD"]);
        let diverged = state(&two);
        assert_eq!((diverged.ahead, diverged.behind), (1, 1));
        assert!(run(&two, "main", Move::Pull).is_some_and(|code| code != 0));
        assert_eq!(git_out(&two, &["rev-parse", "HEAD"]), head);
        assert!(!two.join("c.txt").exists());

        // Behind the remote, a push is rejected and never forced.
        assert!(run(&two, "main", Move::Push).is_some_and(|code| code != 0));
        let remote = root.join("origin.git");
        assert!(!git_out(&remote, &["log", "--format=%s", "main"]).contains("d.txt"));

        // A first push sets the upstream.
        git(&two, &["checkout", "-b", "feature/x"]);
        commit(&two, "e.txt");
        assert_eq!(state(&two).upstream, None);
        assert_eq!(run(&two, "feature/x", Move::Push), Some(0));
        let pushed = state(&two);
        assert_eq!(pushed.upstream.as_deref(), Some("origin/feature/x"));
        assert_eq!((pushed.ahead, pushed.behind), (0, 0));

        // A stale branch name changes nothing, and a detached HEAD is named.
        assert_eq!(run(&two, "main", Move::Push), Some(10));
        git(&two, &["checkout", "--detach"]);
        assert_eq!(state(&two).branch, None);
        assert_eq!(run(&two, "main", Move::Pull), Some(9));
        assert_eq!(
            sh(&move_script("/no/such/folder", "main", Move::Pull)).0,
            Some(3)
        );
    }

    #[test]
    fn a_failed_sign_in_points_at_settings() {
        let why = move_failure(
            Some(128),
            "fatal: Authentication failed for 'https://host/r.git/'",
        );
        assert!(why.contains("Settings"));
        assert_eq!(
            move_failure(Some(128), "fatal: Not possible to fast-forward, aborting."),
            "fatal: Not possible to fast-forward, aborting."
        );
    }

    #[test]
    fn the_scripts_quote_what_they_are_given() {
        assert!(move_script("/srv/my project", "-x", Move::Push).contains("b=-x;"));
        assert!(state_script("/srv/my project").contains("'/srv/my project'"));
    }
}
