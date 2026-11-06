//! Which runs of each project were told, kept in one small file so a
//! restart tells none again.

use anyhow::Context;
use beep_proto::WorkflowRun;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// A run first seen already over is told when it ended this recently: it
/// ended while the watch was moving here.
const FRESH: Duration = Duration::from_secs(10 * 60);
/// The most run ids kept per project. A list reads the 20 newest, so an id
/// older than these is never listed again.
const KEEP: usize = 100;

/// What the watch remembers, by project.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Books {
    #[serde(default)]
    projects: BTreeMap<String, Book>,
}

/// One project's runs.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
struct Book {
    /// Runs over: told, or too old to tell.
    #[serde(default)]
    told: BTreeSet<u64>,
    /// Runs seen going and not over yet.
    #[serde(default)]
    going: BTreeSet<u64>,
}

/// What a list says to do.
#[derive(Debug, Default)]
pub(super) struct Seen {
    /// Runs that ended, to tell.
    pub(super) tell: Vec<WorkflowRun>,
    /// The user's runs still going, to wait on.
    pub(super) going: Vec<WorkflowRun>,
}

/// Whether `run` is one the user started: a push or a pull request by
/// `login`. A host that names no actor (an older Forgejo) counts as the user.
pub(super) fn is_users(run: &WorkflowRun, login: &str) -> bool {
    let event = run.event.as_str();
    let started = event == "push" || event.starts_with("pull_request");
    started && (run.actor.is_empty() || run.actor.eq_ignore_ascii_case(login))
}

impl Books {
    /// Reads the file at `path`; a missing or unreadable one starts empty.
    pub(super) async fn load(path: &Path) -> Self {
        tokio::fs::read(path)
            .await
            .map_err(anyhow::Error::from)
            .and_then(|bytes| Ok(serde_json::from_slice(&bytes)?))
            .unwrap_or_else(|e| {
                log::debug!("CI runs start with nothing told: {e:#}");
                Self::default()
            })
    }

    /// Writes the file, through a partial one so a reader never sees half.
    pub(super) async fn save(&self, path: &Path) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec(self)?;
        let mut partial: PathBuf = path.to_owned();
        partial.as_mut_os_string().push(".part");
        tokio::fs::write(&partial, &bytes)
            .await
            .with_context(|| format!("writing {}", partial.display()))?;
        tokio::fs::rename(&partial, path)
            .await
            .with_context(|| format!("replacing {}", path.display()))
    }

    /// Takes the newest runs of `project`, as listed at `now`, and says
    /// which of the user's ended and which still go.
    pub(super) fn listed(
        &mut self,
        project: &str,
        runs: &[WorkflowRun],
        login: &str,
        now: SystemTime,
    ) -> Seen {
        let mut seen = Seen::default();
        for run in runs.iter().filter(|run| is_users(run, login)) {
            if self.settle(project, run, now) {
                seen.tell.push(run.clone());
            } else if run.state.going() && !self.is_over(project, run.id) {
                seen.going.push(run.clone());
            }
        }
        seen
    }

    /// Takes one read of `run`: true when it ended and is to be told now.
    /// A run is told once, whatever reads it again.
    pub(super) fn settle(&mut self, project: &str, run: &WorkflowRun, now: SystemTime) -> bool {
        let book = self.projects.entry(project.to_owned()).or_default();
        if book.told.contains(&run.id) {
            return false;
        }
        if run.state.going() {
            book.going.insert(run.id);
            trim(&mut book.going);
            return false;
        }
        if !run.state.finished() {
            return false;
        }
        let was_going = book.going.remove(&run.id);
        book.told.insert(run.id);
        trim(&mut book.told);
        was_going || run.finished_at.is_some_and(|at| fresh(at, now))
    }

    fn is_over(&self, project: &str, id: u64) -> bool {
        self.projects
            .get(project)
            .is_some_and(|book| book.told.contains(&id))
    }

    /// Forgets the projects not in `keep`, as when they are unpinned.
    pub(super) fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.projects.retain(|name, _| keep(name));
    }
}

/// Whether a run that ended at `at` ended lately. A host clock ahead of
/// this one counts as lately.
fn fresh(at: SystemTime, now: SystemTime) -> bool {
    now.duration_since(at).map_or(true, |age| age <= FRESH)
}

/// Keeps the `KEEP` newest ids: run ids grow.
fn trim(ids: &mut BTreeSet<u64>) {
    while ids.len() > KEEP {
        ids.pop_first();
    }
}
