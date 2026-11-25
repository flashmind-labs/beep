//! A selected ignored folder has its own file dataset and never owns Git state.

use super::{
    gitdir::{Checkout, Repo, WriteGuard},
    rules::Rules,
};
use crate::files::{
    Folders,
    place::{make_dirs_at, open_target},
};
use anyhow::{Context, Result, ensure};
use beep_proto::node::sync::{IGNORED_PREFIX, ignored_folder};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub(super) struct IgnoredRoot {
    pub(super) repository: PathBuf,
    pub(super) relative: PathBuf,
}

impl IgnoredRoot {
    pub(super) fn parse(name: &str, folder: &Path) -> Result<Option<Self>> {
        if !name.starts_with(IGNORED_PREFIX) {
            return Ok(None);
        }

        let (_, path) = ignored_folder(name).context("Invalid ignored-folder dataset.")?;
        let relative = PathBuf::from(path);
        ensure!(
            folder.ends_with(&relative),
            "The ignored folder does not match its selected path."
        );
        let repository = folder
            .ancestors()
            .nth(relative.components().count())
            .context("The repository root is missing.")?
            .to_owned();
        Ok(Some(Self {
            repository,
            relative,
        }))
    }

    /// Checked before each scan and again under Git's index lock before writes.
    pub(super) fn check(&self) -> Result<()> {
        ensure!(
            !self
                .repository
                .join(".git/beep-recovery/pending.json")
                .exists(),
            "Ignored-folder sync is waiting for Git recovery to finish or be reviewed."
        );
        let git = fs::symlink_metadata(self.repository.join(".git"))
            .context("Waiting for the main Git checkout before syncing ignored files")?;
        ensure!(
            git.is_dir() && !git.file_type().is_symlink(),
            "Ignored-folder sync requires the main Git checkout."
        );
        let mut at = self.repository.clone();

        for part in self.relative.components() {
            at.push(part);

            if let Ok(meta) = fs::symlink_metadata(&at) {
                ensure!(
                    meta.is_dir() && !meta.file_type().is_symlink(),
                    "Ignored folders must be plain directories, without links."
                );
                ensure!(
                    !at.join(".git").exists(),
                    "Nested repositories, worktrees and submodules stay on their machine."
                );
            }
        }

        let repo = Repo::open(&self.repository).context("The Git checkout is unavailable.")?;
        let path = self.relative.to_string_lossy().replace('\\', "/");
        let directory = if self.repository.join(&self.relative).is_dir() {
            path.clone()
        } else {
            format!("{path}/")
        };
        let ignored = repo
            .git()
            .args(["check-ignore", "--quiet", "--no-index", "--", &directory])
            .output()?;
        ensure!(
            ignored.status.success(),
            "{path} is no longer wholly ignored by Git; ignored-folder sync is waiting."
        );
        let literal = format!(":(literal){path}");
        let index = repo.run(&["ls-files", "--cached", "--", &literal])?;
        ensure!(
            index.stdout.is_empty(),
            "{path} contains tracked or staged files; ignored-folder sync is waiting."
        );

        if repo.head().is_some() {
            let head = repo.run(&["ls-tree", "-r", "--name-only", "HEAD", "--", &literal])?;
            ensure!(
                head.stdout.is_empty(),
                "{path} contains committed files; ignored-folder sync is waiting."
            );
        }

        ensure!(
            !Rules::load(&self.repository, "")?.skips_any(&self.relative, true),
            "{path} is excluded by Beep's sync rules."
        );
        Ok(())
    }

    pub(super) fn guard(&self) -> Result<WriteGuard> {
        let guard = WriteGuard::acquire(&self.repository, Some(Checkout::read(&self.repository)))?;
        self.check()?;
        Ok(guard)
    }

    pub(super) fn prepare(&self, folders: &Folders) -> Result<bool> {
        // No missing repository or mount point is recreated by this setting.
        let parent = open_target(folders, &self.repository.to_string_lossy())?;
        let _guard = self.guard()?;
        let made = !self.repository.join(&self.relative).exists();
        make_dirs_at(parent.dir(), &self.relative).context("Making the selected ignored folder")?;
        self.check()?;
        Ok(made)
    }

    pub(super) fn rules(&self, folder: &Path) -> Result<Rules> {
        let mut rules = Rules::load(folder, "")?;
        rules.ignored_within(self.relative.clone(), Rules::load(&self.repository, "")?);
        Ok(rules)
    }

    pub(super) fn watch_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.repository.clone(), self.repository.join(".git")];
        if self.repository.join(".git/info").is_dir() {
            dirs.push(self.repository.join(".git/info"));
        }
        if self.repository.join(".git/beep-recovery").is_dir() {
            dirs.push(self.repository.join(".git/beep-recovery"));
        }

        let mut prefix = self.repository.clone();

        for component in self.relative.components() {
            prefix.push(component);
            if prefix.is_dir() {
                dirs.push(prefix.clone());
            }
        }

        dirs
    }
}

#[cfg(test)]
#[path = "../../tests/files_sync_ignored.rs"]
mod tests;
