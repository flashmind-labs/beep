//! In-memory project state: sessions to workers, build-edit flags, issue mirroring, the
//! lifecycle schedule and its jobs.

use super::{
    lifecycle,
    model::{ProjectId, WorkerId},
    policy::{self, Verdict, WorkerScope},
    schedule::{self, Due, Schedule},
};
use crate::{
    account::user::User,
    app::App,
    lock,
    nodes::{News, Post},
    protocol::node::agent::Question,
};
use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use futures::{FutureExt, future::BoxFuture};
use iroh::EndpointId;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    future::Future,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use strum::Display;
use tokio::{
    sync::{Notify, mpsc, watch},
    task::JoinSet,
    time::timeout,
};
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;

/// Lifecycle jobs running at once, and waiting; past that a job is dropped and re-armed
/// [`DROPPED_RETRY`] later.
const JOBS_RUNNING: usize = 16;
const JOBS_QUEUED: usize = 64;
const DROPPED_RETRY: TimeDelta = TimeDelta::seconds(30);
/// The most jobs one worker keeps waiting; more are dropped.
const WORKER_JOBS: usize = 64;

/// Whether the projects took a session's news.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Handled {
    Yes,
    No,
}

/// What a lifecycle job does, for the logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub(crate) enum JobKind {
    #[strum(to_string = "startup")]
    Startup,
    #[strum(to_string = "worker report")]
    News,
    #[strum(to_string = "worker catch-up")]
    CatchUp,
    #[strum(to_string = "pull request check")]
    Check,
    #[strum(to_string = "idle close")]
    IdleClose,
    #[strum(to_string = "re-attach")]
    Reattach,
    #[strum(to_string = "worker queue")]
    WorkerQueue,
}

type Run = Box<dyn FnOnce(Arc<App>) -> BoxFuture<'static, Result<()>> + Send>;

/// One piece of lifecycle work, run with the app once it starts.
pub(crate) struct Job {
    kind: JobKind,
    run: Run,
}

impl Job {
    pub(crate) fn new<F, Fut>(kind: JobKind, job: F) -> Self
    where
        F: FnOnce(Arc<App>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self {
            kind,
            run: Box::new(move |app| Box::pin(job(app))),
        }
    }
}

/// The projects' shared state, one per server.
#[derive(Default)]
pub struct Projects {
    /// Workers by the machine and node session they run in.
    sessions: Mutex<HashMap<(EndpointId, String), WorkerId>>,
    /// Workers whose next build or test asks first.
    build_edited: Mutex<HashSet<WorkerId>>,
    /// The issue mirroring of each project, the last one started.
    mirrors: Mutex<HashMap<ProjectId, Mirror>>,
    /// Each worker's last handled turn and question, so a report seen twice is handled once.
    seen: Mutex<HashMap<WorkerId, Seen>>,
    schedule: Mutex<Schedule>,
    /// Wakes the schedule's runner when something is armed.
    armed: Arc<Notify>,
    /// Runs the schedule; set by [`super::start`].
    timer: Mutex<Option<AbortOnDropHandle<()>>>,
    jobs: Mutex<Option<Jobs>>,
    /// Each worker's jobs, run in order; present while a run is queued, running or armed.
    worker_jobs: Mutex<HashMap<WorkerId, VecDeque<Job>>>,
    /// What [`Self::post`] posted, for the tests to wait on.
    #[cfg(test)]
    posted: Mutex<Vec<Post>>,
    #[cfg(test)]
    posted_signal: Notify,
    /// Shell text run before each node command, such as a `PATH` with a fake `gh`.
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

    /// Forgets everything kept about `worker`, including its idle close.
    pub(crate) fn untrack(&self, worker: WorkerId) {
        lock(&self.sessions).retain(|_, tracked| *tracked != worker);
        lock(&self.build_edited).remove(&worker);
        lock(&self.seen).remove(&worker);
        self.disarm(&Due::Idle(worker));
    }

    /// Forgets a removed project's check, and its issue mirroring once finished (cutting it off
    /// could leave an issue half made).
    pub(crate) fn forget_project(&self, project: ProjectId) {
        self.disarm(&Due::Check(project));
        lock(&self.mirrors).retain(|id, mirror| *id != project || !*mirror.done.borrow());
    }

    /// Whether `turn` of `worker` is new, marking it handled; `stored` is the store's count.
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

    /// Unmarks question `id` of `worker` so it is handled again next time.
    pub(crate) fn forget_question(&self, worker: WorkerId, id: u32) {
        if let Some(seen) = lock(&self.seen).get_mut(&worker)
            && seen.question == Some(id)
        {
            seen.question = None;
        }
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

    /// Runs `job` in the background, bounded, logging its error; false when the queue was full.
    /// It gets the app only when it starts, so a waiting job does not keep the app alive.
    pub(crate) fn spawn(&self, app: &Arc<App>, job: Job) -> bool {
        let mut jobs = lock(&self.jobs);
        let jobs = jobs.get_or_insert_with(|| {
            let (queue, waiting) = mpsc::channel(JOBS_QUEUED);
            let task = tokio::spawn(run_jobs(Arc::downgrade(app), waiting));
            Jobs {
                queue,
                _task: AbortOnDropHandle::new(task),
            }
        });
        match jobs.queue.try_send(job) {
            Ok(()) => true,
            Err(error) => {
                let kind = error.into_inner().kind;
                log::warn!("projects: dropped a {kind} job, too many waiting");
                false
            }
        }
    }

    /// Runs what is due, re-arming it a little later when the queue is full.
    pub(crate) fn run_due(&self, app: &Arc<App>, due: Due, now: DateTime<Utc>) {
        let queued = match due {
            Due::Worker(worker) => self.spawn_worker(app, worker),
            Due::Check(_) => self.spawn(
                app,
                Job::new(JobKind::Check, move |app| async move {
                    lifecycle::on_due(&app, due, Utc::now()).await
                }),
            ),
            // Behind the worker's reports, so it sees the latest of them.
            Due::Idle(worker) => {
                let job = Job::new(JobKind::IdleClose, move |app| async move {
                    lifecycle::on_due(&app, due, Utc::now()).await
                });
                self.spawn_for(app, worker, job);
                true
            }
        };
        if !queued {
            self.arm(due, now + DROPPED_RETRY);
        }
    }

    /// Queues `job` behind `worker`'s others, so its jobs run in order.
    pub(crate) fn spawn_for(&self, app: &Arc<App>, worker: WorkerId, job: Job) {
        let first = {
            let mut waiting = lock(&self.worker_jobs);
            let first = !waiting.contains_key(&worker);
            let jobs = waiting.entry(worker).or_default();
            if jobs.len() >= WORKER_JOBS {
                log::warn!(
                    "projects: dropped a {} job, too many waiting for its worker",
                    job.kind
                );
                return;
            }
            jobs.push_back(job);
            first
        };
        if first && !self.spawn_worker(app, worker) {
            self.arm(Due::Worker(worker), Utc::now() + DROPPED_RETRY);
        }
    }

    /// Queues a run of `worker`'s waiting jobs.
    fn spawn_worker(&self, app: &Arc<App>, worker: WorkerId) -> bool {
        self.spawn(
            app,
            Job::new(JobKind::WorkerQueue, move |app| async move {
                run_worker(&app, worker).await;
                Ok(())
            }),
        )
    }

    /// The next of `worker`'s jobs, forgetting the worker when none waits.
    fn next_for(&self, worker: WorkerId) -> Option<Job> {
        let mut waiting = lock(&self.worker_jobs);
        let job = waiting.get_mut(&worker)?.pop_front();
        if job.is_none() {
            waiting.remove(&worker);
        }
        job
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
        let job = Job::new(JobKind::News, move |app| async move {
            lifecycle::on_news(&app, worker, news, waited).await
        });
        self.spawn_for(app, worker, job);
        Handled::Yes
    }

    /// `machine` connected again: its workers report again, or are gone.
    pub(crate) fn reconnected(&self, app: &Arc<App>, user: &Arc<User>, machine: EndpointId) {
        let user = user.clone();
        // Dropped, the workers wait for the machine's next reconnect.
        self.spawn(
            app,
            Job::new(JobKind::Reattach, move |app| async move {
                lifecycle::reconnected(&app, &user, machine).await
            }),
        );
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

    /// [`policy::judge`], holding builds back after a build file edit until [`Self::allowed`].
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

    /// The user allowed `question`: an allowed build or test lets the next ones run again.
    pub(crate) fn allowed(&self, worker: WorkerId, question: &Question) {
        if policy::runs_build(question) {
            lock(&self.build_edited).remove(&worker);
        }
    }

    /// Runs `project`'s issue mirroring unless one already runs, waiting at most `budget`; true if
    /// it finished in time. Past the budget it keeps going so no issue is left half made.
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

/// Runs `worker`'s jobs in order until none waits; a failed or panicked job does not stop the rest.
async fn run_worker(app: &Arc<App>, worker: WorkerId) {
    while let Some(job) = app.projects.next_for(worker) {
        let kind = job.kind;
        match AssertUnwindSafe((job.run)(app.clone()))
            .catch_unwind()
            .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => log::warn!("projects: a {kind} job failed: {error:#}"),
            Err(_) => log::warn!("projects: a {kind} job panicked"),
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
                let kind = job.kind;
                let run = (job.run)(strong);
                running.spawn(async move { (kind, run.await) });
            }
            Some(done) = running.join_next() => match done {
                Ok((_, Ok(()))) => {}
                Ok((kind, Err(error))) => log::warn!("projects: a {kind} job failed: {error:#}"),
                Err(error) => log::warn!("projects: a lifecycle job stopped: {error}"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::watchers::testing::{Harness, Scripted};
    use crate::{
        memory::leak::PublicText,
        projects::model::{MachinePath, RepoName, Visibility},
    };
    use anyhow::anyhow;
    use tokio::sync::{Semaphore, oneshot};
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

    #[tokio::test]
    async fn jobs_dropped_for_a_full_queue_are_armed_again() {
        let h = Harness::new(Scripted::default()).await;
        let (app, projects) = (&h.app, &h.app.projects);
        let gate = Arc::new(Semaphore::new(0));
        let (done, mut finished) = mpsc::unbounded_channel();
        let tries = JOBS_RUNNING + JOBS_QUEUED + 8;
        let mut queued = 0;
        for _ in 0..tries {
            let (gate, done) = (gate.clone(), done.clone());
            let job = Job::new(JobKind::Check, move |_| async move {
                // Closed, the gate lets everyone through.
                drop(gate.acquire().await);
                done.send(()).map_err(|_| anyhow!("the test stopped"))
            });
            if projects.spawn(app, job) {
                queued += 1;
            }
        }
        assert!(queued < tries, "{queued} of {tries} queued");
        let now = Utc::now();
        let check = Due::Check(ProjectId(Uuid::new_v4()));
        projects.run_due(app, check, now);
        assert_eq!(projects.armed_at(&check), Some(now + DROPPED_RETRY));
        // A worker's report waits in its worker's queue, which is armed.
        let worker = WorkerId(Uuid::new_v4());
        let (ran, report) = oneshot::channel();
        let job = Job::new(JobKind::News, move |_| async move {
            ran.send(()).map_err(|()| anyhow!("the test stopped"))
        });
        projects.spawn_for(app, worker, job);
        let retry = projects
            .armed_at(&Due::Worker(worker))
            .expect("the worker's jobs are armed");
        gate.close();
        for _ in 0..queued {
            finished.recv().await.expect("a job finishes");
        }
        for due in projects.take_due(retry) {
            projects.run_due(app, due, retry);
        }
        report.await.expect("the report ran");
    }

    #[tokio::test]
    async fn a_workers_jobs_run_in_order() {
        let h = Harness::new(Scripted::default()).await;
        let (app, projects) = (&h.app, &h.app.projects);
        let worker = WorkerId(Uuid::new_v4());
        let order = Arc::new(Mutex::new(Vec::new()));
        let (started, first_started) = oneshot::channel();
        let (release, gate) = oneshot::channel::<()>();
        let (done, second_done) = oneshot::channel();
        let first = {
            let order = order.clone();
            Job::new(JobKind::News, move |_| async move {
                started.send(()).map_err(|()| anyhow!("the test stopped"))?;
                gate.await?;
                lock(&order).push(1);
                Ok(())
            })
        };
        let second = {
            let order = order.clone();
            Job::new(JobKind::IdleClose, move |_| async move {
                lock(&order).push(2);
                done.send(()).map_err(|()| anyhow!("the test stopped"))
            })
        };
        projects.spawn_for(app, worker, first);
        projects.spawn_for(app, worker, second);
        first_started.await.expect("the first job starts");
        release.send(()).expect("the first job waits");
        second_done.await.expect("the second job runs");
        assert_eq!(*lock(&order), vec![1, 2]);
    }
}
