//! Applying received branch histories without switching a user's checkout.

use super::shared::Shared;
use crate::files::sync::gitdir::{self, BranchTip, Repo};
use anyhow::{Context, Result, anyhow, bail, ensure};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::atomic::Ordering,
};
use tokio::task::spawn_blocking;

#[derive(Default)]
pub(super) struct Alignment {
    pub(super) checkout_changed: bool,
    pub(super) warning: Option<anyhow::Error>,
    /// Sessions running in the checkout keep the warning; they end without
    /// a change the task hears of.
    pub(super) held: bool,
}

impl Shared {
    /// Only the project task advances branches, so its own scans and writes cannot overlap.
    pub(super) async fn align_git(&self) -> Result<Alignment> {
        if self.copies_git() {
            return Ok(Alignment::default());
        }

        let own = self.head().await?;
        let catalogs: HashMap<_, _> = crate::lock(&self.peer_branches)
            .iter()
            .filter(|(peer, _)| self.has_peer(peer))
            .map(|(peer, branches)| (*peer, branches.clone()))
            .collect();
        let legacy: Vec<_> = crate::lock(&self.peer_heads)
            .iter()
            .filter(|(peer, head)| {
                self.has_peer(peer) && !catalogs.contains_key(*peer) && own.differs(head.0)
            })
            .map(|(_, head)| head.clone())
            .collect();
        if catalogs.is_empty() && legacy.is_empty() {
            return Ok(Alignment::default());
        }

        let Some(repo) = Repo::open(self.root()) else {
            // Ignored-folder datasets and non-Git projects have no branch histories.
            return Ok(Alignment::default());
        };
        let can_recover = self.recovery_enabled();
        let root = self.root().to_owned();
        let (repo, branch, target, warning) = spawn_blocking(move || -> Result<_> {
            repo.check_recovery()?;

            if gitdir::is_busy(&root) {
                bail!("{}", gitdir::busy_reason(&root));
            }

            let branch = repo.head_ref();
            let mut candidates = BTreeMap::new();
            let mut warning = None;
            let mut blocked = None;
            let mut ahead = false;

            for BranchTip { name, commit } in catalogs.into_values().flatten() {
                candidates.entry(name).or_insert_with(Vec::new).push(commit);
            }

            for (checkout, peer_branch) in legacy {
                if peer_branch != branch {
                    blocked.get_or_insert_with(|| anyhow!("Git commits differ between branches"));
                    continue;
                }

                if let (Some(branch), Some(commit)) = (&branch, checkout.commit) {
                    candidates
                        .entry(branch.clone())
                        .or_insert_with(Vec::new)
                        .push(commit);
                }
            }

            let mut target = None;

            if own.commit.is_none() && branch.as_ref().is_some_and(|name| candidates.contains_key(name)) {
                warning = Some(anyhow!("Git commits differ; checkout has no commit; pull through Git to create its working tree"));
            }

            for local in repo.local_branches()? {
                let Some(commits) = candidates.get(&local.name) else {
                    continue;
                };
                let next = match repo.forward_target(&local.name, local.commit, commits) {
                    Ok(Some(target)) => target,
                    Ok(None) => {
                        if branch.as_deref() == Some(local.name.as_str())
                            && commits.iter().any(|commit| *commit != local.commit) {
                            ahead = true;
                        }
                        continue;
                    }
                    Err(e) => {
                        warning.get_or_insert(e);
                        continue;
                    }
                };
                if branch.as_deref() == Some(local.name.as_str()) {
                    target = Some(next);
                } else if let Err(e) = repo.advance_inactive(&local.name, next) {
                    blocked.get_or_insert(e);
                }
            }

            let warning = warning.or(blocked).or_else(|| ahead.then(|| anyhow!("Git commits differ; this checkout is ahead of the peers")));

            if target.is_some() {
                repo.check_recovery()?;

                if !can_recover {
                    repo.require_clean()?;
                }
            }

            Ok((repo, branch, target, warning))
        })
        .await??;
        let Some(target) = target else {
            return Ok(Alignment {
                checkout_changed: false,
                warning,
                held: false,
            });
        };
        let branch = branch.context("Git checkout changed while aligning its branches")?;
        let dirty = {
            let root = self.root().to_owned();
            spawn_blocking(move || {
                Repo::open(&root)
                    .context("Git checkout is unavailable")?
                    .has_local_changes()
            })
            .await??
        };
        let hold = if dirty {
            ensure!(
                self.recovery_enabled(),
                "Local edits block Git sync; enable automatic recovery or commit or stash them."
            );

            match self.occupancy.reserve(self.root()).await {
                Ok(hold) => Some(hold),
                Err(e) => {
                    return Ok(Alignment {
                        checkout_changed: false,
                        warning: Some(e),
                        held: true,
                    });
                }
            }
        } else {
            None
        };
        let (snapshot, file) = {
            let mut state = self.lock();
            state.saved.fast_forward = Some(target);
            state.unsaved = true;
            (state.saved.clone(), self.file.clone())
        };
        let policy = self.recovery.clone();

        if dirty {
            *crate::write_lock(&self.bootstrap_paths) = Some(HashSet::new());
        }

        spawn_blocking(move || {
            // Cancellation must not release the session hold during stash/restore.
            let _hold = hold;
            anyhow::ensure!(
                !dirty || policy.load(Ordering::SeqCst),
                "Automatic Git recovery was disabled; local edits stay in place."
            );
            crate::fs::note::write(&file, &rmp_serde::to_vec(&snapshot)?)?;
            if dirty {
                repo.recover_fast_forward(own, target, &branch)
            } else {
                repo.fast_forward(own, target, &branch)
            }
        })
        .await??;
        self.pull_again();
        Ok(Alignment {
            checkout_changed: true,
            warning,
            held: false,
        })
    }
}
