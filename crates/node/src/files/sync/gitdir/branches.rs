//! Branch advertisements, peer references and inactive-branch fast-forwards.

use super::{Oid, Repo, WriteGuard, is_busy};
use anyhow::{Context, Result, bail, ensure};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const HEADS: &str = "refs/heads/";
const MAX_BRANCHES: usize = 4096;
const MAX_BRANCH_NAME: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::files::sync) struct BranchTip {
    pub(in crate::files::sync) name: String,
    pub(in crate::files::sync) commit: Oid,
}

impl BranchTip {
    pub(in crate::files::sync) fn validate(tips: &[Self]) -> Result<()> {
        ensure!(tips.len() <= MAX_BRANCHES, "Too many Git branches to sync");
        let mut names = HashSet::new();

        for tip in tips {
            ensure!(
                tip.name.starts_with(HEADS)
                    && tip.name.len() > HEADS.len()
                    && tip.name.len() <= MAX_BRANCH_NAME
                    && names.insert(&tip.name),
                "Invalid or duplicate Git branch advertisement"
            );
        }

        Ok(())
    }
}

impl Repo {
    /// The tips of the branches that sync: the default branch and the one
    /// checked out here. Branches of linked worktrees stay on this machine.
    pub(in crate::files::sync) fn branches(&self) -> Result<Vec<BranchTip>> {
        let (default, head) = (self.default_branch(), self.head_ref());
        let mut tips = self.local_branches()?;
        tips.retain(|tip| tip.name == default || head.as_ref() == Some(&tip.name));
        Ok(tips)
    }

    /// Every local branch tip, excluding stashes, recovery refs and peer refs.
    pub(in crate::files::sync) fn local_branches(&self) -> Result<Vec<BranchTip>> {
        let tips = self
            .refs()?
            .into_iter()
            .filter_map(|reference| {
                let name = reference.found.path.strip_prefix(".git/")?;
                name.starts_with(HEADS).then(|| BranchTip {
                    name: name.to_owned(),
                    commit: reference.oid,
                })
            })
            .collect::<Vec<_>>();
        BranchTip::validate(&tips)?;
        Ok(tips)
    }

    /// The commits to tell `peer` this checkout has when it packs `branch`,
    /// the closest first: the branch here, HEAD, the default branch, then
    /// the branches it sent before.
    pub(in crate::files::sync) fn haves(&self, peer: &EndpointId, branch: &str) -> Vec<Oid> {
        let received = self.cached_branches(peer).unwrap_or_default();
        let candidates = [
            self.ref_oid(branch),
            self.head(),
            self.ref_oid(&self.default_branch()),
        ]
        .into_iter()
        .flatten()
        .chain(received.into_iter().map(|tip| tip.commit));
        let mut seen = HashSet::new();
        candidates.filter(|oid| seen.insert(*oid)).collect()
    }

    /// Whether a peer's branch `name` is one to take: the default branch, or
    /// one this checkout has too. A peer's other branches cost no transfer.
    pub(in crate::files::sync) fn takes_branch(&self, name: &str) -> bool {
        name == self.default_branch() || self.ref_oid(name).is_some()
    }

    /// Whether local branch `branch` reaches `commit`.
    fn holds(&self, branch: &str, commit: Oid) -> bool {
        self.ref_oid(branch)
            .is_some_and(|tip| self.is_ancestor(commit, tip) == Some(true))
    }

    /// The full name of `origin`'s default branch, or `main` when `origin`
    /// names none.
    fn default_branch(&self) -> String {
        let name = self
            .gix()
            .ok()
            .and_then(|repo| crate::files::git_read::default_branch(&repo));
        format!("{HEADS}{}", name.as_deref().unwrap_or("main"))
    }

    pub(in crate::files::sync) fn validate_branch(&self, name: &str) -> Result<()> {
        ensure!(
            name.starts_with(HEADS) && name.len() <= MAX_BRANCH_NAME,
            "Only local Git branches may be requested"
        );
        gix::refs::FullName::try_from(name).context("Invalid Git branch name")?;
        Ok(())
    }

    /// The object ref `name` points at, without peeling a tag.
    pub(in crate::files::sync) fn ref_oid(&self, name: &str) -> Option<Oid> {
        let repo = self.gix().ok()?;
        let mut reference = repo.try_find_reference(name).ok()??;
        let id = reference.follow_to_object().ok()?;
        Some(Oid::from(id.as_ref()))
    }

    pub(in crate::files::sync) fn peer_ref(peer: &EndpointId, branch: &str) -> Result<String> {
        let branch = branch.strip_prefix(HEADS).context("Not a Git branch")?;
        Ok(format!("refs/remotes/beep/{peer}/{branch}"))
    }

    pub(in crate::files::sync) fn cached_branches(
        &self,
        peer: &EndpointId,
    ) -> Result<Vec<BranchTip>> {
        let prefix = format!(".git/refs/remotes/beep/{peer}/");
        let tips = self
            .refs()?
            .into_iter()
            .filter_map(|reference| {
                let branch = reference.found.path.strip_prefix(&prefix)?;
                Some(BranchTip {
                    name: format!("{HEADS}{branch}"),
                    commit: reference.oid,
                })
            })
            .collect::<Vec<_>>();
        BranchTip::validate(&tips)?;
        Ok(tips)
    }

    fn retain_peer_history(&self, peer: &EndpointId, branch: &str, commit: Oid) -> Result<()> {
        let retained = format!(
            "refs/beep-sync/retained/{peer}/{}/{}",
            blake3::hash(branch.as_bytes()).to_hex(),
            commit
        );

        if self.ref_oid(&retained) != Some(commit) {
            self.set_ref(&retained, commit, None)?;
        }

        Ok(())
    }

    /// Removed peer refs are archived, never local branches. This also clears name-prefix
    /// conflicts when a peer replaces a branch such as `topic` with `topic/next`. Returns
    /// the tips whose peer ref is missing or elsewhere: a tip already held needs no Git.
    pub(in crate::files::sync) fn prepare_peer_refs(
        &self,
        peer: &EndpointId,
        tips: &[BranchTip],
    ) -> Result<Vec<BranchTip>> {
        let prefix = format!("refs/remotes/beep/{peer}/");
        let held = self
            .refs()?
            .into_iter()
            .filter_map(|reference| {
                let name = reference.found.path.trim_start_matches(".git/");
                let branch = name.strip_prefix(&prefix)?;
                Some((format!("{HEADS}{branch}"), reference.oid))
            })
            .collect::<HashMap<_, _>>();

        let changed = tips
            .iter()
            .filter(|tip| held.get(&tip.name) != Some(&tip.commit))
            .cloned()
            .collect::<Vec<_>>();

        for tip in changed.iter().filter(|tip| !held.contains_key(&tip.name)) {
            self.validate_branch(&tip.name)?;
        }

        let wanted = tips.iter().map(|tip| &tip.name).collect::<HashSet<_>>();

        for (branch, &oid) in held.iter().filter(|(branch, _)| !wanted.contains(branch)) {
            // The peer removed the branch or stopped sharing it. Its history
            // is kept only for a branch taken here that does not hold it yet.
            if self.takes_branch(branch) && !self.holds(branch, oid) {
                self.retain_peer_history(peer, branch, oid)?;
            }

            self.delete_ref(&Self::peer_ref(peer, branch)?, oid)?;
        }

        Ok(changed)
    }

    /// Pin received commits before the peer's advertisement is accepted by the task.
    pub(in crate::files::sync) fn keep_peer_branch(
        &self,
        peer: &EndpointId,
        tip: &BranchTip,
    ) -> Result<()> {
        self.validate_branch(&tip.name)?;
        ensure!(
            self.is_complete(tip.commit),
            "Peer Git history is incomplete"
        );
        let reference = Self::peer_ref(peer, &tip.name)?;
        let old = self.ref_oid(&reference);

        if old == Some(tip.commit) {
            return Ok(());
        }
        // A peer can rewind or replace its branch. Retain the old received history too.
        if let Some(old) = old
            && self.is_ancestor(old, tip.commit) != Some(true)
        {
            self.retain_peer_history(peer, &tip.name, old)?;
        }

        self.set_ref(&reference, tip.commit, old)
    }

    /// The newest peer commit reachable without merging or dropping local history.
    pub(in crate::files::sync) fn forward_target(
        &self,
        branch: &str,
        own: Oid,
        candidates: &[Oid],
    ) -> Result<Option<Oid>> {
        let mut target = own;

        for &candidate in candidates {
            if self.is_ancestor(candidate, target) == Some(true) {
                continue;
            }

            match self.is_ancestor(target, candidate) {
                Some(true) => target = candidate,
                Some(false) => bail!(
                    "Git commits differ; histories diverged for {branch}; merge the peer commits to resume sync"
                ),
                None => {
                    bail!("Git commits differ; waiting for peer commits for {branch} to arrive")
                }
            }
        }

        Ok((target != own).then_some(target))
    }

    /// Git itself refuses to fetch into a branch checked out in any worktree.
    /// This also covers a checkout that starts after the caller read HEAD.
    pub(in crate::files::sync) fn advance_inactive(&self, branch: &str, target: Oid) -> Result<()> {
        self.validate_branch(branch)?;
        self.check_recovery()?;
        ensure!(!is_busy(&self.root), "Waiting for the local Git operation");
        let _guard = WriteGuard::for_branches(&self.root)?;
        ensure!(
            self.head_ref().as_deref() != Some(branch) && !self.is_linked_to(branch),
            "Git commits differ; branch {branch} is checked out in a worktree; its received commits are retained"
        );
        self.run(&[
            "fetch",
            "--no-tags",
            "--no-prune",
            "--no-write-fetch-head",
            "--no-auto-maintenance",
            "--no-recurse-submodules",
            ".",
            &format!("{target}:{branch}"),
        ])
        .with_context(|| format!("Waiting to fast-forward inactive Git branch {branch}"))?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../../tests/files_sync_gitdir_branches.rs"]
mod tests;
