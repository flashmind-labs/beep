//! What the server keeps in memory about projects: which node session is
//! which worker, which workers edited a build file since a build was last
//! allowed, and the issue mirroring running for each project.

use super::{
    model::{ProjectId, WorkerId},
    policy::{self, Verdict, WorkerScope},
};
use crate::protocol::node::agent::Question;
use iroh::EndpointId;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::{sync::watch, time::timeout};
use tokio_util::task::AbortOnDropHandle;

/// The projects' shared state, one per server.
#[derive(Default)]
pub struct Projects {
    /// Workers by the machine and node session they run in.
    sessions: Mutex<HashMap<(EndpointId, String), WorkerId>>,
    /// Workers whose next build or test asks first.
    build_edited: Mutex<HashSet<WorkerId>>,
    /// The issue mirroring of each project, the last one started.
    mirrors: Mutex<HashMap<ProjectId, Mirror>>,
}

struct Mirror {
    done: watch::Receiver<bool>,
    _task: AbortOnDropHandle<()>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Projects {
    /// The worker running in `session` on `machine`.
    pub(crate) fn worker_of(&self, machine: &EndpointId, session: &str) -> Option<WorkerId> {
        lock(&self.sessions)
            .get(&(*machine, session.to_owned()))
            .copied()
    }

    pub(crate) fn track(&self, machine: EndpointId, session: &str, worker: WorkerId) {
        lock(&self.sessions).insert((machine, session.to_owned()), worker);
    }

    /// Forgets `worker`: its session and whether it edited a build file.
    pub(crate) fn untrack(&self, worker: WorkerId) {
        lock(&self.sessions).retain(|_, tracked| *tracked != worker);
        lock(&self.build_edited).remove(&worker);
    }

    /// Judges what `worker` asks to do, as [`policy::judge`] does, holding
    /// its builds and tests back from the moment it edits a build file
    /// until the user allows one (see [`Self::allowed`]).
    pub(crate) fn judge(
        &self,
        worker: WorkerId,
        question: &Question,
        scope: &WorkerScope<'_>,
    ) -> Verdict {
        let build_edited = lock(&self.build_edited).contains(&worker);
        let verdict = policy::judge(
            question,
            &WorkerScope {
                build_edited,
                ..*scope
            },
        );
        if policy::edits_build_file(question) {
            lock(&self.build_edited).insert(worker);
        }
        verdict
    }

    /// The user allowed `question` of `worker`'s: a build or test lets the
    /// next ones run as before.
    pub(crate) fn allowed(&self, worker: WorkerId, question: &Question) {
        if policy::runs_build(question) {
            lock(&self.build_edited).remove(&worker);
        }
    }

    /// Runs `work`, the issue mirroring of `project`, in the background,
    /// unless an earlier one still runs, and waits at most `budget` for
    /// whichever runs. Past the budget it keeps going: an issue create cut
    /// off half way may leave an issue nobody saved. Says whether it
    /// finished in time.
    pub(crate) async fn mirror(
        &self,
        project: ProjectId,
        budget: Duration,
        work: impl Future<Output = ()> + Send + 'static,
    ) -> bool {
        let mut done = {
            let mut mirrors = lock(&self.mirrors);
            match mirrors.get(&project) {
                Some(running) if !*running.done.borrow() => running.done.clone(),
                _ => {
                    let (finished, done) = watch::channel(false);
                    let task = tokio::spawn(async move {
                        work.await;
                        // Nobody waiting is fine: the entry keeps a receiver.
                        let _ = finished.send(true);
                    });
                    mirrors.insert(
                        project,
                        Mirror {
                            done: done.clone(),
                            _task: AbortOnDropHandle::new(task),
                        },
                    );
                    done
                }
            }
        };
        // A mirror that panicked drops its sender, which ends the wait too.
        timeout(budget, done.wait_for(|done| *done)).await.is_ok()
    }

    /// Waits for `project`'s mirroring to finish.
    #[cfg(test)]
    pub(crate) async fn mirrored(&self, project: ProjectId) {
        let done = lock(&self.mirrors)
            .get(&project)
            .map(|mirror| mirror.done.clone());
        if let Some(mut done) = done {
            let _ = done.wait_for(|done| *done).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        leak::PublicText,
        projects::model::{MachinePath, RepoName, Visibility},
    };
    use iroh::SecretKey;
    use uuid::Uuid;

    fn ask(text: &str, detail: &str) -> Question {
        Question {
            id: 1,
            text: text.to_owned(),
            detail: Some(detail.to_owned()),
            options: Vec::new(),
        }
    }

    #[test]
    fn a_build_file_edit_holds_builds_until_one_is_allowed() {
        let repo = RepoName::parse("acme/widget").expect("a repo");
        let worktree = MachinePath::parse("/srv/project/.worktrees/fix-it").expect("a path");
        let public_text = PublicText::new(&[], &[]);
        let scope = WorkerScope {
            repo: &repo,
            worktree: &worktree,
            branch: "beep/fix-it",
            default_branch: "main",
            visibility: Visibility::Private,
            outside_allowed: false,
            public_text: &public_text,
            build_edited: false,
        };
        let projects = Projects::default();
        let (worker, other) = (WorkerId(Uuid::new_v4()), WorkerId(Uuid::new_v4()));
        let test = ask("Run a command", "cargo test");
        assert_eq!(projects.judge(worker, &test, &scope), Verdict::Allow);
        let edit = ask("Write file", "crates/x/build.rs");
        assert_eq!(projects.judge(worker, &edit, &scope), Verdict::Allow);
        assert_eq!(projects.judge(worker, &test, &scope), Verdict::AskUser);
        // Asking again does not let it through, nor does allowing a non-build.
        projects.allowed(worker, &ask("Run a command", "curl x"));
        assert_eq!(projects.judge(worker, &test, &scope), Verdict::AskUser);
        assert_eq!(projects.judge(other, &test, &scope), Verdict::Allow);
        projects.allowed(worker, &test);
        assert_eq!(projects.judge(worker, &test, &scope), Verdict::Allow);
        // Ending the worker forgets it.
        projects.judge(worker, &edit, &scope);
        projects.untrack(worker);
        assert_eq!(projects.judge(worker, &test, &scope), Verdict::Allow);
    }

    #[test]
    fn sessions_map_to_workers() {
        let projects = Projects::default();
        let machine = SecretKey::from_bytes(&[1; 32]).public();
        let worker = WorkerId(Uuid::new_v4());
        projects.track(machine, "s1", worker);
        assert_eq!(projects.worker_of(&machine, "s1"), Some(worker));
        assert_eq!(projects.worker_of(&machine, "s2"), None);
        projects.untrack(worker);
        assert_eq!(projects.worker_of(&machine, "s1"), None);
    }
}
