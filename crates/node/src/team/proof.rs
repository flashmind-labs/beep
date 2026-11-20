//! Proof of push access: the joiner pushes `refs/beep/proof/<nonce>`, the
//! inviter looks for it with `git ls-remote` and deletes it.

use std::{fmt, path::Path, process::Stdio};
use tokio::io::AsyncWriteExt;

/// A random value that names one proof ref.
pub(crate) struct Nonce([u8; 16]);

impl Nonce {
    pub(crate) fn generate() -> Self {
        Self(rand::random())
    }
}

impl fmt::Display for Nonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ProofError {
    #[error("You need push access to {0}")]
    Denied(String),
    #[error("Your forge refused the proof ref")]
    RefRefused,
    #[error("git: {0}")]
    Git(String),
}

/// Pushes an empty commit from `checkout` to the proof ref of `nonce`.
pub(crate) async fn push_proof(
    checkout: &Path,
    remote: &str,
    nonce: &Nonce,
) -> Result<(), ProofError> {
    let tree = git(checkout, &["mktree"], Some("")).await?;
    let commit = git(
        checkout,
        &[
            "-c",
            "user.name=beep",
            "-c",
            "user.email=beep@localhost",
            "commit-tree",
            tree.trim(),
            "-m",
            "beep team proof",
        ],
        None,
    )
    .await?;
    let refspec = format!("{}:{}", commit.trim(), proof_ref(nonce));

    git(checkout, &["push", remote, &refspec], None)
        .await
        .map(drop)
        .map_err(|error| classify(error, remote))
}

/// Whether the remote has the proof ref of `nonce`.
pub(crate) async fn proof_seen(
    checkout: &Path,
    remote: &str,
    nonce: &Nonce,
) -> Result<bool, ProofError> {
    let seen = git(checkout, &["ls-remote", remote, &proof_ref(nonce)], None).await?;

    Ok(!seen.trim().is_empty())
}

/// Deletes the proof ref of `nonce` from the remote.
pub(crate) async fn drop_proof(
    checkout: &Path,
    remote: &str,
    nonce: &Nonce,
) -> Result<(), ProofError> {
    let refspec = format!(":{}", proof_ref(nonce));

    git(checkout, &["push", remote, &refspec], None)
        .await
        .map(drop)
}

fn proof_ref(nonce: &Nonce) -> String {
    format!("refs/beep/proof/{nonce}")
}

/// Sorts a failed push by what git's stderr says.
fn classify(error: ProofError, remote: &str) -> ProofError {
    let ProofError::Git(stderr) = error else {
        return error;
    };
    let said = stderr.to_lowercase();

    if ["permission", "403", "denied"]
        .iter()
        .any(|word| said.contains(word))
    {
        ProofError::Denied(remote.to_owned())
    } else if ["refusing", "not allowed", "hook declined", "invalid ref"]
        .iter()
        .any(|word| said.contains(word))
    {
        ProofError::RefRefused
    } else {
        ProofError::Git(stderr)
    }
}

/// What `git <args>` printed in `dir`, with `stdin` fed to it; stderr when it failed.
async fn git(dir: &Path, args: &[&str], stdin: Option<&str>) -> Result<String, ProofError> {
    let mut command = tokio::process::Command::from(crate::run::process::background_command("git"));
    command
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| ProofError::Git(error.to_string()))?;

    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(text.as_bytes())
            .await
            .map_err(|error| ProofError::Git(error.to_string()))?;
    }

    let out = child
        .wait_with_output()
        .await
        .map_err(|error| ProofError::Git(error.to_string()))?;
    if !out.status.success() {
        return Err(ProofError::Git(
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        ));
    }

    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
#[path = "../tests/team_proof.rs"]
mod tests;
