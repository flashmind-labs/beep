//! The copy of a project's workflow runs, and the live progress of the
//! running ones: their jobs are read again only while a run is in progress
//! and someone has the page open, at most every 10 seconds, and not while
//! the token's rate limit runs low.

use super::{
    actions::{PrHeadRef, pr_checks, progress},
    http::Who,
    runs::{self, Target, target},
    watch::{Entry, RepoWatch},
};
use crate::{
    app::App,
    credentials::host_client,
    git_items::HostError,
    protocol::{ProjectRunDetail, ProjectRuns, RunJob, RunLog, RunState, WorkflowRun},
    store::ProjectRow,
    user::User,
};
use reqwest::Client;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Instant, SystemTime},
};

/// Running runs whose jobs one poll reads.
const MAX_LIVE: usize = 4;
/// Failed jobs' logs kept per project.
const MAX_LOGS: usize = 8;

/// The last runs read, the jobs read of each, and the logs read.
#[derive(Default)]
pub(crate) struct RunsCopy {
    runs: Vec<WorkflowRun>,
    /// By run id; `None` when the host has no API for jobs.
    jobs: HashMap<u64, Option<Vec<RunJob>>>,
    error: Option<String>,
    synced_at: Option<SystemTime>,
    /// Whose rate limit the last reads used, to check it without the
    /// target.
    who: Option<Who>,
    /// Failed jobs' logs by job id, oldest first.
    logs: VecDeque<(u64, RunLog)>,
}

impl RunsCopy {
    fn limited(&self, watch: &RepoWatch) -> Option<SystemTime> {
        let who = self.who.as_ref()?;
        watch.calls.limited_until(who, SystemTime::now())
    }

    /// The runs with the progress of each running one.
    fn shown(&self) -> Vec<WorkflowRun> {
        self.runs
            .iter()
            .map(|run| {
                let mut run = run.clone();
                if run.state.going()
                    && let Some(Some(jobs)) = self.jobs.get(&run.id)
                {
                    run.progress = progress(jobs);
                }
                run
            })
            .collect()
    }
}

/// A target and a client to call it with.
struct Reach {
    target: Target,
    client: Client,
}

async fn reach(app: &App, user: &User, project: &ProjectRow) -> Result<Reach, String> {
    let target = target(app, user, project).await?;
    let client = host_client().map_err(|e| {
        log::warn!("repo watch: no client for the host: {e:#}");
        HostError::Internal(target.host.clone()).to_string()
    })?;
    Ok(Reach { target, client })
}

impl RepoWatch {
    /// `project`'s runs from the copy: the list read again as `lists` reads
    /// its lists, and the running runs' jobs while the page asks.
    pub(crate) async fn runs(
        &self,
        app: &Arc<App>,
        user: &Arc<User>,
        project: &ProjectRow,
        refresh: bool,
    ) -> ProjectRuns {
        let entry = self.entry(user.id(), project.id);
        let ask = entry.runs.ask(Instant::now(), refresh);
        let mut copy = entry.runs.copy.lock().await;
        let limited = copy.limited(self);
        let read_list = ask.forced || (entry.runs.stale() && limited.is_none());
        let running = copy.runs.iter().any(|run| run.state.going());
        let poll = running && limited.is_none() && entry.runs.timing().poll(Instant::now());
        if read_list || poll {
            match reach(app, user, project).await {
                Err(why) => {
                    copy.error = Some(why);
                    entry.runs.timing().synced(Instant::now(), false);
                }
                Ok(reach) => {
                    copy.who = Some(reach.target.who.clone());
                    // The list again too while polling: unchanged, it is a
                    // 304 GitHub does not count, and it says when a run ends.
                    self.read_list(app, &reach, &entry, &mut copy).await;
                    if poll || copy.runs.iter().any(|run| run.state.going()) {
                        self.read_running(app, &reach, &mut copy).await;
                    }
                }
            }
        }
        let shown = copy.shown();
        let heads = entry.heads();
        let refs: Vec<PrHeadRef<'_>> = heads
            .iter()
            .map(|head| PrHeadRef {
                number: head.number,
                head: &head.branch,
            })
            .collect();
        ProjectRuns {
            checks: pr_checks(&shown, &refs),
            runs: shown,
            synced_at: copy.synced_at,
            error: copy.error.clone(),
            rate_limited_until: copy.limited(self),
        }
    }

    async fn read_list(&self, app: &App, reach: &Reach, entry: &Entry, copy: &mut RunsCopy) {
        let caller = reach.target.caller(app, &reach.client);
        match runs::list(&self.calls, &caller, &reach.target).await {
            Ok(runs) => {
                copy.jobs
                    .retain(|id, _| runs.iter().any(|run| run.id == *id));
                copy.runs = runs;
                copy.error = None;
                copy.synced_at = Some(SystemTime::now());
                entry.runs.timing().synced(Instant::now(), true);
            }
            Err(e) => {
                copy.error = Some(e.to_string());
                entry.runs.timing().synced(Instant::now(), false);
            }
        }
    }

    /// The jobs of the running runs, the newest few.
    async fn read_running(&self, app: &App, reach: &Reach, copy: &mut RunsCopy) {
        let caller = reach.target.caller(app, &reach.client);
        let running: Vec<u64> = copy
            .runs
            .iter()
            .filter(|run| run.state.going())
            .take(MAX_LIVE)
            .map(|run| run.id)
            .collect();
        for id in running {
            match runs::jobs(&self.calls, &caller, &reach.target, id).await {
                Ok(jobs) => {
                    copy.jobs.insert(id, jobs);
                }
                // The progress shown stays the last read; the list says
                // why when the host is down.
                Err(e) => log::info!("repo watch: jobs of a run did not read: {e}"),
            }
        }
    }

    /// One run with its jobs and, when it failed, its failing job's log.
    pub(crate) async fn run_detail(
        &self,
        app: &Arc<App>,
        user: &Arc<User>,
        project: &ProjectRow,
        id: u64,
    ) -> Result<ProjectRunDetail, String> {
        let entry = self.entry(user.id(), project.id);
        entry.runs.ask(Instant::now(), false);
        let mut copy = entry.runs.copy.lock().await;
        let reach = reach(app, user, project).await?;
        copy.who = Some(reach.target.who.clone());
        let caller = reach.target.caller(app, &reach.client);
        let target = &reach.target;
        let limited = copy.limited(self);
        let polled = limited.is_none() && entry.runs.timing().poll(Instant::now());
        let mut problems = Vec::new();

        let known = copy.runs.iter().find(|run| run.id == id).cloned();
        let run = match known {
            Some(run) if !(run.state.going() && polled) => run,
            known => match runs::one(&self.calls, &caller, target, id).await {
                Ok(run) => {
                    if let Some(at) = copy.runs.iter_mut().find(|r| r.id == id) {
                        *at = run.clone();
                    }
                    run
                }
                Err(e) => match known {
                    Some(run) => {
                        problems.push(e.to_string());
                        run
                    }
                    None => return Err(e.to_string()),
                },
            },
        };

        let cached = copy.jobs.get(&id).cloned();
        let stale_jobs = |jobs: &Option<Vec<RunJob>>| {
            jobs.as_ref()
                .is_some_and(|jobs| jobs.iter().any(|job| job.state.going()))
        };
        let read_jobs = match &cached {
            None => limited.is_none(),
            Some(jobs) => stale_jobs(jobs) && (polled || run.state.finished()) && limited.is_none(),
        };
        let jobs = if read_jobs {
            match runs::jobs(&self.calls, &caller, target, id).await {
                Ok(jobs) => {
                    copy.jobs.insert(id, jobs.clone());
                    jobs
                }
                Err(e) => {
                    problems.push(e.to_string());
                    cached.flatten()
                }
            }
        } else {
            cached.flatten()
        };
        let jobs = jobs.unwrap_or_default();

        let log = if run.state == (RunState::Failed {}) {
            match jobs.iter().find(|job| job.state == RunState::Failed {}) {
                Some(job) => match copy.logs.iter().find(|(at, _)| *at == job.id) {
                    Some((_, log)) => Some(log.clone()),
                    None if limited.is_some() => None,
                    None => {
                        let step = job
                            .steps
                            .iter()
                            .find(|step| step.state == RunState::Failed {})
                            .map(|step| step.name.clone());
                        match runs::log(&self.calls, &caller, target, job, step).await {
                            Ok(log) => {
                                if copy.logs.len() >= MAX_LOGS {
                                    copy.logs.pop_front();
                                }
                                copy.logs.push_back((job.id, log.clone()));
                                Some(log)
                            }
                            Err(e) => {
                                problems.push(format!("The log did not read: {e}"));
                                None
                            }
                        }
                    }
                },
                None => None,
            }
        } else {
            None
        };

        let mut run = run;
        run.progress = if run.state.going() {
            progress(&jobs)
        } else {
            None
        };
        Ok(ProjectRunDetail {
            run,
            jobs,
            log,
            error: (!problems.is_empty()).then(|| problems.join(" ")),
            rate_limited_until: copy.limited(self),
        })
    }
}

/// The background sync's read of the run list, unless an ask read it
/// meanwhile or the token's rate limit runs low.
pub(super) async fn sync_in_background(
    app: &Arc<App>,
    user: &Arc<User>,
    project: &ProjectRow,
    entry: &Entry,
) {
    let mut copy = entry.runs.copy.lock().await;
    if !entry.runs.stale() || copy.limited(&app.repo_watch).is_some() {
        return;
    }
    match reach(app, user, project).await {
        Ok(reach) => {
            copy.who = Some(reach.target.who.clone());
            app.repo_watch
                .read_list(app, &reach, entry, &mut copy)
                .await;
        }
        Err(why) => {
            copy.error = Some(why);
            entry.runs.timing().synced(Instant::now(), false);
        }
    }
}
