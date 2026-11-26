//! A durable local stash before an opted-in fast-forward. Backups are never dropped here.

use super::{Checkout, GIT, Oid, Repo, is_busy};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
enum Phase {
    Preparing,
    Stashed,
    Restoring,
}

#[derive(Debug, Serialize, Deserialize)]
struct Recovery {
    reference: String,
    before: Checkout,
    target: Oid,
    phase: Phase,
    stash: Option<Oid>,
}

impl Repo {
    fn recovery_file(&self) -> PathBuf {
        self.root.join(GIT).join("beep-recovery/pending.json")
    }

    pub(in crate::files::sync) fn has_recovery_backups(&self) -> bool {
        self.root.join(GIT).join("beep-recovery").exists()
    }

    pub(in crate::files::sync) fn check_recovery(&self) -> Result<()> {
        match fs::read(self.recovery_file()) {
            Ok(bytes) => {
                let recovery: Recovery = serde_json::from_slice(&bytes)
                    .context("Git sync recovery needs review; its recovery record is unreadable")?;
                bail!(
                    "Git sync recovery needs review after {:?}. Local work is retained in {} or refs/stash. Inspect the checkout and backup, resolve any conflicts, then remove .git/beep-recovery/pending.json only after verifying local work is preserved.",
                    recovery.phase,
                    recovery.reference
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => {
                Err(e).context("Git sync recovery needs review; its recovery record cannot be read")
            }
        }
    }

    pub(in crate::files::sync) fn recover_fast_forward(
        &self,
        before: Checkout,
        target: Oid,
        branch: &str,
    ) -> Result<()> {
        self.check_recovery()?;
        ensure!(
            !is_busy(&self.root),
            "Git is busy; automatic recovery will retry later."
        );
        ensure!(
            Checkout::read(&self.root) == before && self.head_ref().as_deref() == Some(branch),
            "The Git checkout changed before recovery; retrying sync."
        );
        ensure!(
            before
                .commit
                .is_some_and(|own| self.is_ancestor(own, target) == Some(true)),
            "Git commits differ; automatic recovery only allows a fast-forward."
        );

        if !self.has_local_changes()? {
            return self.fast_forward(before, target, branch);
        }

        let file = self.recovery_file();
        fs::create_dir_all(file.parent().context("Recovery directory is missing")?)?;
        let mut recovery = Recovery {
            reference: format!("refs/beep-recovery/{}", Uuid::new_v4()),
            before,
            target,
            phase: Phase::Preparing,
            stash: None,
        };
        // Written before stash can remove anything. A restart never repeats a partial recovery.
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)?;
        out.write_all(&serde_json::to_vec(&recovery)?)?;
        out.sync_all()?;
        drop(out);
        let result = (|| -> Result<()> {
            let previous = self.stash_oid();
            self.run(&[
                "stash",
                "push",
                "-u",
                "-m",
                &format!("Beep Git sync recovery {}", recovery.reference),
            ])?;
            let stash = self
                .stash_oid()
                .filter(|stash| Some(*stash) != previous)
                .context("Git did not create a new recovery stash")?;
            let message = self.run(&["show", "-s", "--format=%s", &stash.to_string()])?;
            ensure!(
                String::from_utf8_lossy(&message.stdout).contains(&recovery.reference),
                "The stash changed during recovery; inspect its reflog before restoring work."
            );
            recovery.stash = Some(stash);
            self.set_ref(&recovery.reference, stash, None)?;
            recovery.phase = Phase::Stashed;
            crate::fs::note::write_json(&file, &recovery)?;
            self.fast_forward(before, target, branch)?;
            recovery.phase = Phase::Restoring;
            crate::fs::note::write_json(&file, &recovery)?;
            self.run(&["stash", "apply", "--index", &stash.to_string()])?;
            ensure!(
                self.head() == Some(target) && self.head_ref().as_deref() == Some(branch),
                "The checkout changed while restoring local work"
            );
            fs::remove_file(&file)?;
            Ok(())
        })();
        result.with_context(|| format!("Git sync recovery needs review. Local work remains in {} or refs/stash. Inspect and restore the backup before removing .git/beep-recovery/pending.json", recovery.reference))
    }

    fn stash_oid(&self) -> Option<Oid> {
        let out = self.run(&["rev-parse", "--verify", "refs/stash"]).ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }
}
