//! In-memory project state: issue mirroring, the
//! lifecycle schedule and its jobs.

use super::{
    lifecycle,
    model::{ProjectId, WorkerId},
    schedule::{self, Due, Schedule},
};
use crate::{
    account::user::User,
    app::{App, Draws, Message},
    lock,
    protocol::DeviceKind,
    store::CreateRun,
};
use anyhow::{Result, bail};
use chrono::{DateTime, TimeDelta, Utc};
use futures::{FutureExt, future::BoxFuture};
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use strum::Display;
use tokio::{
    sync::{Notify, mpsc, watch},
    task::JoinSet,
    time::{Instant, sleep, timeout},
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
/// How long a report waits for its busy chat, and how often it tries when no turn end woke it.
const POST_PATIENCE: Duration = Duration::from_secs(30 * 60);
const POST_RETRY: Duration = Duration::from_secs(5);

/// What a lifecycle job does, for the logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub(crate) enum JobKind {
    #[strum(to_string = "startup")]
    Startup,
    #[strum(to_string = "chat report")]
    Post,
    #[strum(to_string = "pull request check")]
    Check,
    #[strum(to_string = "idle close")]
    IdleClose,
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
    /// The issue mirroring of each project, the last one started.
    mirrors: Mutex<HashMap<ProjectId, Mirror>>,
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
    posted: Mutex<Vec<(String, String)>>,
    #[cfg(test)]
    posted_signal: Notify,
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
    /// Forgets everything kept about `worker`, including its idle close.
    pub(crate) fn untrack(&self, worker: WorkerId) {
        self.disarm(&Due::Idle(worker));
    }

    /// Forgets a removed project's check, and its issue mirroring once finished (cutting it off
    /// could leave an issue half made).
    pub(crate) fn forget_project(&self, project: ProjectId) {
        self.disarm(&Due::Check(project));
        lock(&self.mirrors).retain(|id, mirror| *id != project || !*mirror.done.borrow());
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
            lock(&self.posted).push((label.to_owned(), text.clone()));
            self.posted_signal.notify_waiters();
        }
        let label = label.to_owned();
        self.spawn(
            app,
            Job::new(JobKind::Post, move |app| async move {
                deliver(&app, &user, chat, &label, text).await
            }),
        );
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

/// Starts a turn of `chat` on `text` once the chat is free, giving up after [`POST_PATIENCE`].
async fn deliver(
    app: &Arc<App>,
    user: &Arc<User>,
    chat: Uuid,
    label: &str,
    text: String,
) -> Result<()> {
    let deadline = Instant::now() + POST_PATIENCE;
    loop {
        // Registered before trying, so a turn ending in between still wakes it.
        let ended = app.subagents.turn_ended.notified();
        tokio::pin!(ended);
        ended.as_mut().enable();
        match app
            .submit(user, posted_message(chat, label, text.clone()))
            .await?
        {
            CreateRun::Conflict if Instant::now() < deadline => {}
            CreateRun::Conflict => bail!("chat {chat} stayed busy; the report was dropped"),
            CreateRun::Created(_) | CreateRun::Existing(_) => return Ok(()),
        }
        tokio::select! {
            () = ended => {}
            () = sleep(POST_RETRY) => {}
        }
    }
}

/// A turn the project starts in its chat, labelled `label`.
fn posted_message(chat: Uuid, label: &str, text: String) -> Message {
    Message {
        session_id: chat,
        request_id: Uuid::new_v4(),
        message: text,
        cron_id: None,
        scheduled: Some(label.to_owned()),
        always_sent: true,
        woken: None,
        images: Vec::new(),
        files: Vec::new(),
        answer: None,
        model: None,
        thinking: None,
        origin: DeviceKind::Phone {},
        platform: None,
        draws: Draws::default(),
        finished_research: true,
        voice: None,
        spoken: false,
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
#[path = "../tests/projects_service.rs"]
mod tests;
