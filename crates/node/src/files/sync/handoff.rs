//! Explicitly bringing uncommitted edits to another checkout.

use super::{
    Syncs,
    gitdir::{self, Checkout, Repo, WriteGuard},
};
use anyhow::{Context, Result, ensure};
use beep_proto::node::sync::HandoffSnapshot;
use std::{fs, path::Path, process::Stdio};
use tokio::task::spawn_blocking;

const MAX_PATCH: u64 = 4 * 1024 * 1024;

fn snapshot(root: &Path) -> Result<HandoffSnapshot> {
    ensure!(
        !gitdir::is_busy(root),
        "Finish the Git operation before handing off work."
    );
    let repo = Repo::open(root).context("This folder is not a Git checkout.")?;
    let base = repo
        .head()
        .context("Commit the initial version before handing off work.")?
        .to_string();
    ensure!(
        repo.run(&["ls-files", "--unmerged"])?.stdout.is_empty(),
        "Resolve Git conflicts before handing off work."
    );
    let status = repo.run(&["status", "--porcelain=v2", "-z", "--ignore-submodules=none"])?;

    for entry in status.stdout.split(|byte| *byte == 0) {
        if matches!(entry.first(), Some(b'1' | b'2')) {
            let submodule = entry.split(|byte| *byte == b' ').nth(2).unwrap_or_default();
            ensure!(
                submodule.first() != Some(&b'S') || submodule.get(1..) == Some(b"..."),
                "Submodule changes stay on this machine. Commit and push them separately before handing off work."
            );
        }
    }
    // A separate index includes tracked and untracked edits without staging anything for the user.
    let temp = tempfile::tempdir()?;
    let index = temp.path().join("index");
    // Start from the real index so force-added ignored files are included too.
    let source_index = root.join(".git/index");

    if source_index.exists() {
        fs::copy(source_index, &index)?;
    }

    if !index.exists() {
        let output = repo
            .git()
            .env("GIT_INDEX_FILE", &index)
            .args(["read-tree", "HEAD"])
            .output()?;
        ensure!(
            output.status.success(),
            "Git could not capture uncommitted work."
        );
    }

    let output = repo
        .git()
        .env("GIT_INDEX_FILE", &index)
        .args(["add", "--update", "--", "."])
        .output()?;
    ensure!(
        output.status.success(),
        "Git could not capture tracked edits: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    // Embedded repositories are listed as directories and stay on their machine.
    let untracked = repo.run(&["ls-files", "--others", "--exclude-standard", "-z"])?;
    let files: Vec<_> = untracked
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| {
            !path.is_empty()
                && !path.ends_with(b"/")
                && *path != b".beep-sync"
                && *path != b".worktrees"
                && !path.starts_with(b".worktrees/")
        })
        .map(std::str::from_utf8)
        .collect::<std::result::Result<_, _>>()?;
    for batch in files.chunks(128) {
        let output = repo
            .git()
            .env("GIT_INDEX_FILE", &index)
            .arg("--literal-pathspecs")
            .args(["add", "--"])
            .args(batch)
            .output()?;
        ensure!(
            output.status.success(),
            "Git could not capture untracked edits: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let patch = temp.path().join("changes.patch");
    let result = repo
        .git()
        .env("GIT_INDEX_FILE", &index)
        .args([
            "diff",
            "--cached",
            "--binary",
            "--full-index",
            "--no-ext-diff",
            "--no-textconv",
            "HEAD",
            "--",
        ])
        .stdout(Stdio::from(fs::File::create(&patch)?))
        .output()?;
    ensure!(
        result.status.success(),
        "Git could not capture uncommitted work."
    );
    ensure!(
        fs::metadata(&patch)?.len() <= MAX_PATCH,
        "Uncommitted work exceeds the 4 MiB handoff limit. Commit large files first."
    );
    ensure!(
        repo.head().is_some_and(|head| head.to_string() == base),
        "The source commit changed. Try the handoff again."
    );
    Ok(HandoffSnapshot {
        base,
        patch: fs::read(patch)?,
    })
}

fn apply(root: &Path, snapshot: HandoffSnapshot) -> Result<()> {
    ensure!(
        u64::try_from(snapshot.patch.len())? <= MAX_PATCH,
        "The handoff exceeds 4 MiB."
    );
    let repo = Repo::open(root).context("This folder is not a Git checkout.")?;
    ensure!(
        repo.head()
            .is_some_and(|head| head.to_string() == snapshot.base),
        "Checkouts have different commits. Pull the same commit before handing off work."
    );
    repo.require_clean()?;
    let _guard = WriteGuard::acquire(root, Some(Checkout::read(root)))?;
    ensure!(
        repo.head()
            .is_some_and(|head| head.to_string() == snapshot.base),
        "The destination commit changed. Try again."
    );
    ensure!(
        repo.run(&[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignore-submodules=none"
        ])?
        .stdout
        .is_empty(),
        "Commit or stash destination changes before handing off work."
    );

    if snapshot.patch.is_empty() {
        return Ok(());
    }

    let patch = tempfile::NamedTempFile::new()?;
    fs::write(patch.path(), snapshot.patch)?;
    let path = patch
        .path()
        .to_str()
        .context("The temporary path is not UTF-8.")?;
    // Without --reject, Git checks the whole patch before changing any files.
    repo.run(&["apply", "--check", "--binary", "--", path])?;
    repo.run(&["apply", "--binary", "--", path])?;
    Ok(())
}

impl Syncs {
    async fn handoff_root(&self, project: &str) -> Result<std::path::PathBuf> {
        let shared = self.project(project).context("This project is unavailable or managed by Syncthing. Exclude it from Syncthing before handing off work.")?;
        ensure!(
            !shared.copies_git(),
            "Wait for the initial Git checkout to finish before handing off work."
        );
        ensure!(
            !shared.is_paused(),
            "Resume the project's sync before handing off work."
        );
        let root = shared.root().to_owned();
        ensure!(
            self.inner.syncthing.check(&root).await.is_none(),
            "Syncthing still manages this checkout."
        );
        Ok(root)
    }

    pub(crate) async fn handoff_for_move(
        &self,
        project: &str,
        from: &iroh::EndpointId,
    ) -> Result<()> {
        let shared = self
            .project(project)
            .context("This project is not available for handoff.")?;
        if !shared.status().git_only {
            return Ok(());
        }

        let peer = shared
            .watch_peers()
            .borrow()
            .iter()
            .find(|peer| peer.id == *from)
            .cloned()
            .context("The source no longer shares this project.")?;
        let incoming =
            super::peer::handoff_snapshot(&self.inner.parts.endpoint, peer, project).await?;
        let _one = self.inner.configuring.lock().await;
        let root = self.handoff_root(project).await?;
        // A retried move may already have applied this exact snapshot.
        spawn_blocking(move || {
            if snapshot(&root)? == incoming {
                return Ok(());
            }

            apply(&root, incoming)
        })
        .await?
    }

    /// Captures an explicit handoff without changing the source checkout.
    pub async fn handoff_snapshot(&self, project: &str) -> Result<HandoffSnapshot> {
        let _one = self.inner.configuring.lock().await;
        let root = self.handoff_root(project).await?;
        spawn_blocking(move || snapshot(&root)).await?
    }

    /// Applies a handoff only to a clean checkout at its exact base commit.
    pub async fn handoff_apply(&self, project: &str, snapshot: HandoffSnapshot) -> Result<()> {
        let _one = self.inner.configuring.lock().await;
        let root = self.handoff_root(project).await?;
        spawn_blocking(move || apply(&root, snapshot)).await??;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/files_sync_handoff.rs"]
mod tests;
