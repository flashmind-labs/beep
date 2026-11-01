//! What the server keeps in memory about projects: which node session is
//! which worker, which workers edited a build file since a build was last
//! allowed, the issue mirroring running for each project, the lifecycle's
//! schedule and the jobs that carry out what workers report.

use super::{
    lifecycle,
    model::{ProjectId, WorkerId},
    policy::{self, Verdict, WorkerScope},
    schedule::{self, Due, Schedule},
};
use crate::{
    app::App,
    nodes::{News, Post},
    protocol::node::agent::Question,
    user::User,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use iroh::EndpointId;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::{Arc, Mutex, MutexGuard, PoisonError, Weak},
    time::Duration,
};
use tokio::{
    sync::{Notify, mpsc, watch},
    task::JoinSet,
    time::timeout,
};
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;

/// Lifecycle jobs running at once, and waiting to run; more are dropped.
const JOBS_RUNNING: usize = 16;
const JOBS_QUEUED: usize = 64;

/// Whether the projects took a session's news.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Handled {
    Yes,
    No,
}

/// One piece of lifecycle work, run with the app once it starts.
type Job = Box<dyn FnOnce(Arc<App>) -> BoxFuture<'static, Result<()>> + Send>;

/// The projects' shared state, one per server.
#[derive(Default)]
pub struct Projects {
    /// Workers by the machine and node session they run in.
    sessions: Mutex<HashMap<(EndpointId, String), WorkerId>>,
    /// Workers whose next build or test asks first.
    build_edited: Mutex<HashSet<WorkerId>>,
    /// The issue mirroring of each project, the last one started.
    mirrors: Mutex<HashMap<ProjectId, Mirror>>,
    /// The last turn and question of each worker's that was handled, so a
    /// report seen twice (live and on re-attach) is handled once.
    seen: Mutex<HashMap<WorkerId, Seen>>,
    schedule: Mutex<Schedule>,
    /// Wakes the schedule's runner when something is armed.
    armed: Arc<Notify>,
    /// Runs the schedule; set by [`super::start`].
    timer: Mutex<Option<AbortOnDropHandle<()>>>,
    jobs: Mutex<Option<Jobs>>,
    /// What [`Self::post`] posted, for the tests to wait on.
    #[cfg(test)]
    posted: Mutex<Vec<Post>>,
    #[cfg(test)]
    posted_signal: Notify,
    /// Shell text run before each node shell command, such as a `PATH`
    /// with stand-ins for `gh`.
    #[cfg(test)]
    shell_prefix: Mutex<Option<String>>,
}

#[derive(Debug, Default, Clone, Copy)]
struct Seen {
    turn: u32,
    question: Option<u32>,
}

/// The lifecycle's job runner.
struct Jobs {
    queue: mpsc::Sender<Job>,
    _task: AbortOnDropHandle<()>,
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

    /// Forgets `worker`: its session, whether it edited a build file, what
    /// of it was handled and its idle close.
    pub(crate) fn untrack(&self, worker: WorkerId) {
        lock(&self.sessions).retain(|_, tracked| *tracked != worker);
        lock(&self.build_edited).remove(&worker);
        lock(&self.seen).remove(&worker);
        self.disarm(&Due::Idle(worker));
    }

    /// Whether `turn` of `worker`'s is new, noting it handled when it is.
    /// `stored` is how many the store says were heard about.
    pub(crate) fn take_turn(&self, worker: WorkerId, turn: u32, stored: u32) -> bool {
        let mut seen = lock(&self.seen);
        let seen = seen.entry(worker).or_insert(Seen {
            turn: stored,
            question: None,
        });
        seen.turn = seen.turn.max(stored);
        let new = turn > seen.turn;
        if new {
            seen.turn = turn;
        }
        new
    }

    /// Whether question `id` of `worker`'s is new, noting it handled.
    pub(crate) fn take_question(&self, worker: WorkerId, id: u32) -> bool {
        let mut seen = lock(&self.seen);
        let seen = seen.entry(worker).or_default();
        let new = seen.question != Some(id);
        seen.question = Some(id);
        new
    }

    pub(crate) fn arm(&self, due: Due, at: DateTime<Utc>) {
        lock(&self.schedule).arm(due, at);
        self.armed.notify_one();
    }

    pub(crate) fn disarm(&self, due: &Due) {
        lock(&self.schedule).disarm(due);
    }

    pub(crate) fn next_due(&self) -> Option<DateTime<Utc>> {
        lock(&self.schedule).next()
    }

    pub(crate) fn take_due(&self, now: DateTime<Utc>) -> Vec<Due> {
        lock(&self.schedule).take_due(now)
    }

    #[cfg(test)]
    pub(crate) fn armed_at(&self, due: &Due) -> Option<DateTime<Utc>> {
        lock(&self.schedule).armed_at(due)
    }

    /// Starts the schedule's runner, once.
    pub(crate) fn start_timer(&self, app: &Arc<App>) {
        let mut timer = lock(&self.timer);
        if timer.is_none() {
            let task = tokio::spawn(schedule::run(Arc::downgrade(app), self.armed.clone()));
            *timer = Some(AbortOnDropHandle::new(task));
        }
    }

    /// Runs `job` in the background, a bounded number at a time, logging
    /// its error. It gets the app when it starts, so a waiting job does not
    /// keep the app alive.
    pub(crate) fn spawn<F, Fut>(&self, app: &Arc<App>, job: F)
    where
        F: FnOnce(Arc<App>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let job: Job = Box::new(move |app| Box::pin(job(app)));
        let mut jobs = lock(&self.jobs);
        let jobs = jobs.get_or_insert_with(|| {
            let (queue, waiting) = mpsc::channel(JOBS_QUEUED);
            let task = tokio::spawn(run_jobs(Arc::downgrade(app), waiting));
            Jobs {
                queue,
                _task: AbortOnDropHandle::new(task),
            }
        });
        if jobs.queue.try_send(job).is_err() {
            log::warn!("projects: dropped a worker report, too many waiting");
        }
    }

    /// Takes a node session's news when the session is a project worker's.
    pub(crate) fn on_news(
        &self,
        app: &Arc<App>,
        machine: &EndpointId,
        session: &str,
        news: &News,
        waited: bool,
    ) -> Handled {
        let Some(worker) = self.worker_of(machine, session) else {
            return Handled::No;
        };
        let news = news.clone();
        self.spawn(app, move |app| async move {
            lifecycle::on_news(&app, worker, news, waited).await
        });
        Handled::Yes
    }

    /// `machine` connected again: its workers report again, or are gone.
    pub(crate) fn reconnected(&self, app: &Arc<App>, user: &Arc<User>, machine: EndpointId) {
        let user = user.clone();
        self.spawn(app, move |app| async move {
            lifecycle::reconnected(&app, &user, machine).await
        });
    }

    /// Starts a turn of `chat` on `text`, labelled `label`.
    pub(crate) fn post(
        &self,
        app: &Arc<App>,
        user: Arc<User>,
        chat: Uuid,
        label: &str,
        text: String,
    ) {
        #[cfg(test)]
        {
            lock(&self.posted).push(Post {
                user: user.clone(),
                chat,
                label: label.to_owned(),
                text: text.clone(),
            });
            self.posted_signal.notify_waiters();
        }
        app.nodes.deliveries.post(
            app,
            Post {
                user,
                chat,
                label: label.to_owned(),
                text,
            },
        );
    }

    #[cfg(test)]
    pub(crate) fn set_shell_prefix(&self, prefix: &str) {
        *lock(&self.shell_prefix) = Some(prefix.to_owned());
    }

    #[cfg(test)]
    pub(crate) fn shell_prefix(&self) -> Option<String> {
        lock(&self.shell_prefix).clone()
    }

    /// Waits until `done` holds for what was posted, and gives it.
    #[cfg(test)]
    pub(crate) async fn posted_until(
        &self,
        done: impl Fn(&[(String, String)]) -> bool,
    ) -> Vec<(String, String)> {
        loop {
            let signal = self.posted_signal.notified();
            tokio::pin!(signal);
            signal.as_mut().enable();
            let posted: Vec<(String, String)> = lock(&self.posted)
                .iter()
                .map(|post| (post.label.clone(), post.text.clone()))
                .collect();
            if done(&posted) {
                return posted;
            }
            signal.await;
        }
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

/// Runs the jobs, at most [`JOBS_RUNNING`] at once.
async fn run_jobs(app: Weak<App>, mut waiting: mpsc::Receiver<Job>) {
    let mut running = JoinSet::new();
    loop {
        tokio::select! {
            job = waiting.recv(), if running.len() < JOBS_RUNNING => {
                let Some(job) = job else { return };
                let Some(strong) = app.upgrade() else { return };
                running.spawn(job(strong));
            }
            Some(done) = running.join_next() => match done {
                Ok(Ok(())) => {}
                Ok(Err(error)) => log::warn!("projects: a worker job failed: {error:#}"),
                Err(error) => log::warn!("projects: a worker job stopped: {error}"),
            },
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
