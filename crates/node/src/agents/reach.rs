//! Which folders the caller may reach on this node through the file
//! requests (`Reach`, `Access`). Every path is checked with its links and
//! `..` resolved.

use super::{Agents, limits::SIGHTING_WAIT, media_file::resolve, worktrees};
use crate::fs::paths::{self, real};
use crate::run::device::{self, Served};
use beep_proto::node::{LinkRx, LinkTx};
use crate::logs::claude_dir;
use crate::logs::found::LogRoots;
use anyhow::{Context, Result};
use std::time::Duration;
use std::{
    path::{Path, PathBuf},
};
use tokio::{task::spawn_blocking, time::timeout};

/// How long git may take to list the worktrees of the sessions' repositories.
const WORKTREE_WAIT: Duration = Duration::from_secs(5);

/// What a request does with what it names. Read and written: the node's
/// workspace and shared folders, and the folders sessions work in. Only
/// read: the harnesses' session logs and Claude Code's temp folder, where
/// it keeps the files it makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Read,
    Write,
    /// Read, but only in a work folder: a project folder asked for.
    Work,
    /// Read as `Work`, for a checkout's diff.
    Diff,
}

/// The folders a request may reach, as they were named.
#[derive(Debug, Default, Clone)]
pub(super) struct Reach {
    /// Read and written.
    pub(super) work: Vec<PathBuf>,
    /// Only read.
    pub(super) read: Vec<PathBuf>,
    /// Never written, even inside a work folder.
    pub(super) no_write: Vec<PathBuf>,
    /// The folders of the sessions, in `work` too.
    pub(super) sessions: Vec<PathBuf>,
}

impl Reach {
    /// This reach with the checkout of each session folder that is a removed
    /// worktree added to `work`, so the apps can move the session there.
    /// Blocks on git.
    pub(super) fn with_checkouts(mut self) -> Self {
        let checkouts: Vec<PathBuf> = self
            .sessions
            .iter()
            .filter_map(|folder| worktrees::checkout_of_gone(folder))
            .collect();
        self.work.extend(checkouts);
        self
    }

    /// Whether `real`, already resolved by [`real`], lies in a folder
    /// `access` may reach. Blocks on the file system.
    pub(super) fn holds(&self, real: &Path, access: Access) -> bool {
        let inside = |folders: &[PathBuf]| {
            folders
                .iter()
                .filter_map(|folder| paths::canonicalize(folder).ok())
                // A session in `/` would open the whole disk.
                .filter(|folder| folder.parent().is_some())
                .any(|folder| real.starts_with(folder))
        };

        match access {
            Access::Read => inside(&self.work) || inside(&self.read),
            Access::Write => inside(&self.work) && !inside(&self.no_write),
            Access::Work | Access::Diff => inside(&self.work),
        }
    }
}

/// The refusal for `asked`, outside what `access` may reach.
pub(super) fn refused(asked: &str, access: Access) -> anyhow::Error {
    let what = match access {
        Access::Read => "read",
        Access::Write => "write",
        Access::Work => "list custom agents or worktrees in",
        Access::Diff => "read changes in",
    };
    anyhow::anyhow!(
        "{asked} is outside the folders Beep may {what} on this node: its workspace, \
         shared folders and the folders of agent sessions"
    )
}

impl Agents {
    /// The folders the node's sessions work in, once each.
    fn session_folders(&self) -> Vec<PathBuf> {
        let mut folders = self.session_cwds();
        folders.sort();
        folders.dedup();
        folders
    }

    /// The folder of each session this node runs.
    fn session_cwds(&self) -> Vec<PathBuf> {
        self.session_map()
            .values()
            .map(|session| PathBuf::from(session.info().cwd))
            .collect()
    }

    /// The folders of the sessions started outside Beep that run now.
    fn live_presence_folders(&self) -> Vec<PathBuf> {
        self.occupancy
            .presence()
            .snapshot()
            .into_iter()
            .filter(|session| session.presence.is_live())
            .map(|session| PathBuf::from(session.cwd))
            .collect()
    }

    /// What file requests may reach, the sessions' folders among them;
    /// those of sessions started outside Beep only with `sighted`.
    fn reach(&self, sighted: Vec<PathBuf>) -> Reach {
        let mut work: Vec<PathBuf> = self.folders.clone();
        let mut sessions = self.session_cwds();
        sessions.extend(sighted);
        sessions.extend(self.live_presence_folders());
        work.extend(sessions.iter().cloned());
        let logs = LogRoots::locate();
        let (temp, uploads) = (claude_temp(), super::uploads::folder());
        let read = [
            claude_dir::projects_dir(),
            logs.codex.as_ref().map(|codex| codex.join("sessions")),
            logs.codex.map(|codex| codex.join("archived_sessions")),
            logs.pi,
            logs.flsh,
            // OpenCode's database, or its folder of them.
            logs.opencode,
            temp.clone(),
            uploads.clone(),
        ]
        .into_iter()
        .flatten()
        .collect();
        Reach {
            work,
            read,
            no_write: temp.into_iter().chain(uploads).collect(),
            sessions,
        }
    }

    /// Work directories currently authorized for namespace-wide file subscriptions.
    pub(crate) async fn notification_roots(&self) -> Result<Vec<PathBuf>> {
        let mut work = self.folders.clone();
        work.extend(self.session_map().values().filter_map(|session| {
            let info = session.info();
            (info.state.status != beep_proto::node::agent::Status::Ended)
                .then(|| PathBuf::from(info.cwd))
        }));
        work.extend(self.live_presence_folders());
        work.extend(self.observed_work_roots());
        spawn_blocking(move || {
            let mut roots: Vec<_> = work
                .into_iter()
                .filter_map(|root| real(&root).ok())
                .filter(|root| root.parent().is_some() && (root.is_dir() || !root.exists()))
                .collect();
            roots.sort();
            roots.dedup();
            let parents = roots.clone();
            // A worktree inside a checkout stays a root of its own: the
            // checkout's watch leaves out its changes.
            roots.retain(|root| {
                !parents
                    .iter()
                    .any(|parent| crate::events::paths::reports(parent, root))
            });
            Ok(roots)
        })
        .await
        .context("finding notification roots stopped")?
    }

    /// `device/1` for the node and its agents: the stream's requests that
    /// name a folder or file are checked against what the agents may reach
    /// before the device code answers them.
    pub(crate) async fn serve_device(&self, tx: LinkTx, rx: LinkRx) -> Result<()> {
        let mut stream = Served::new(tx, rx);

        while let Some(request) = stream.next().await? {
            let path = match device::named(&request) {
                Some(named) => Some(self.reachable(named, Access::Read).await),
                None => None,
            };

            match stream.answer(request, path).await? {
                Some(more) => stream = more,
                None => break,
            }
        }

        Ok(())
    }

    /// `asked`, absolute or starting `~/`, with its links and `..`
    /// resolved, once it is inside the folders `access` may reach; refused
    /// otherwise.
    pub(crate) async fn reachable(&self, asked: &str, access: Access) -> Result<PathBuf> {
        let home = dirs::home_dir();
        let reach = self.reach(Vec::new());
        let owned = asked.to_owned();
        let (real, held) = spawn_blocking(move || -> Result<(PathBuf, bool)> {
            let real = real(&resolve(&owned, home.as_deref())?)?;
            let reach = reach.with_checkouts();
            let held = reach.holds(&real, access);
            Ok((real, held))
        })
        .await
        .context("the path check stopped")??;

        if held {
            return Ok(real);
        }
        // A worktree of a session's repository outside its folder is read
        // like the folder, never written. Git is asked now, so a path the
        // desktop merely names does not count.
        if access != Access::Write {
            let folders = self.session_folders();
            let asked_real = real.clone();
            let listed = spawn_blocking(move || worktrees::listed_in(&folders, &asked_real));

            if timeout(WORKTREE_WAIT, listed)
                .await
                .is_ok_and(|done| done.unwrap_or(false))
            {
                return Ok(real);
            }
        }
        // The folders of sessions started outside Beep cost a look in the
        // harnesses' logs, so only when the others did not hold it. The
        // look is kept a while and bounded, so a request answers in time.
        let sighted = timeout(SIGHTING_WAIT, self.sighted_folders())
            .await
            .map_err(|_| refused(asked, access))
            .context("the agents' logs took too long to read")??;
        let reach = self.reach(sighted);
        let held =
            spawn_blocking(move || reach.with_checkouts().holds(&real, access).then_some(real))
                .await
                .context("the path check stopped")?;
        held.ok_or_else(|| refused(asked, access))
    }
}

/// Claude Code's temp folder for this user. See [`claude_temp_in`].
pub(super) fn claude_temp() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        let uid = rustix::process::getuid().as_raw();
        Some(claude_temp_in(
            std::env::var_os(CLAUDE_TEMP_VAR).as_deref(),
            uid,
        ))
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// What moves Claude Code's temp folder.
#[cfg(unix)]
const CLAUDE_TEMP_VAR: &str = "CLAUDE_CODE_TMPDIR";

/// Claude Code's temp folder for `uid`: `claude-<uid>` in `set`, the
/// value of `CLAUDE_CODE_TMPDIR`, or else in `/tmp`. Claude Code uses
/// `/tmp` on macOS too, never `TMPDIR` or the per-user
/// `/var/folders/.../T/`, so the folder does not depend on the
/// environment launchd gives the node's service. A `CLAUDE_CODE_TMPDIR`
/// set only in the user's shell is not seen by the service.
#[cfg(unix)]
fn claude_temp_in(set: Option<&std::ffi::OsStr>, uid: u32) -> PathBuf {
    let base = set
        .filter(|base| !base.is_empty())
        .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    base.join(format!("claude-{uid}"))
}

#[cfg(test)]
#[path = "../tests/agents_reach.rs"]
mod tests;
