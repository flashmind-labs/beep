//! A project's CI runs, read from its host's Actions API with the user's token: the list with each
//! running run's progress, how each open pull request's checks stand, and one run's jobs with the
//! end of a failed job's log. The reads and their ETag cache are `beep_projects::repo_watch`'s.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    records::{Stored, find_project, now_ms, time},
};
use beep_projects::{
    credentials::Slot,
    imports::{
        hosts::{credential_of, forgejo_of},
        repo::{Host, RepoRef},
    },
    issues::list_open_prs,
    model::RepoName,
    repo_watch::{
        actions::{PrHeadRef, pr_checks, progress},
        runs::{self as host_runs, Target},
    },
};
use beep_proto::{
    GitHostKind, HostedChecks, HostedRun, HostedRuns, ProjectRunDetail, RunState, RunsHost,
    WorkflowRun,
};
use secrecy::ExposeSecret;

/// What the host's rate limit is shared by: the desktop's calls to it.
const WHO: &str = "desktop";
/// The most running runs whose jobs are read for their progress.
const PROGRESS_READS: usize = 5;
const NO_HOST: &str =
    "This project's repository is not on a GitHub or Forgejo host Beep has a token for.";

/// Where a project's runs are read.
struct Place {
    target: Target,
    kind: GitHostKind,
    repo: String,
}

impl Desk {
    /// The project's host and the token that reads it, or why there is none.
    fn run_place(&self, stored: &Stored) -> std::result::Result<Place, String> {
        let opened = self.opened();
        let repo = RepoRef::parse(&stored.record.url, &forgejo_of(&opened))
            .map_err(|_| NO_HOST.to_owned())?;
        let found = credential_of(&repo, &opened)
            .ok_or_else(|| format!("Beep has no token for {}.", repo.host_name()))?;
        let name = RepoName::parse(&repo.project()).map_err(|e| e.to_string())?;
        let secret = found.secret.expose_secret().to_owned().into();
        let (mut target, kind) = match (&repo.host, &found.slot) {
            (Host::GitHub, Slot::GitHub) => {
                let account = found
                    .github
                    .as_ref()
                    .map_or("", |login| login.login.as_str());
                (
                    Target::github(WHO, name, secret, account),
                    GitHostKind::GitHub {},
                )
            }
            (Host::Forgejo(url), Slot::Forgejo(_)) => (
                Target::forgejo(WHO, name, url, secret),
                GitHostKind::Forgejo {},
            ),
            _ => return Err(NO_HOST.to_owned()),
        };
        if let (Some(base), GitHostKind::GitHub {}) = (&self.github_api, &kind) {
            target.base.clone_from(base);
        }
        Ok(Place {
            target,
            kind,
            repo: repo.project(),
        })
    }

    /// The newest runs of the project, and the checks of its open pull requests.
    pub async fn runs(&self, project: &str) -> Result<HostedRuns> {
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        let place = match self.run_place(&stored) {
            Ok(place) => place,
            Err(why) => {
                return Ok(HostedRuns {
                    runs: Vec::new(),
                    checks: Vec::new(),
                    hosts: Vec::new(),
                    error: Some(why),
                });
            }
        };
        let client = self.hosts.reqwest()?;
        let calls = self.hosts.host_calls();
        let caller = place.target.caller(self.hosts.semaphore(), &client);
        let host = place.target.host.clone();
        let listed = host_runs::list(calls, &caller, &place.target).await;
        let (mut shown, error) = match listed {
            Ok(listed) => (listed, None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        let mut reads = 0;
        for listed in &mut shown {
            if listed.run.state.going() && reads < PROGRESS_READS {
                reads += 1;
                if let Ok(Some(jobs)) =
                    host_runs::jobs(calls, &caller, &place.target, listed.run.id).await
                {
                    listed.run.progress = progress(&jobs);
                }
            }
        }
        let runs: Vec<WorkflowRun> = shown.iter().map(|listed| listed.run.clone()).collect();
        let heads = self.open_heads(project).await;
        let refs: Vec<PrHeadRef<'_>> = heads
            .iter()
            .map(|(number, head)| PrHeadRef {
                number: *number,
                head,
            })
            .collect();
        let checks = pr_checks(&runs, &refs)
            .into_iter()
            .map(|checks| HostedChecks {
                host: host.clone(),
                checks,
            })
            .collect();
        let rate_limited_until =
            calls.limited_until(&place.target.who, std::time::SystemTime::now());
        Ok(HostedRuns {
            runs: runs
                .into_iter()
                .map(|run| HostedRun {
                    host: host.clone(),
                    run,
                })
                .collect(),
            checks,
            hosts: vec![RunsHost {
                host,
                kind: place.kind,
                repo: place.repo,
                primary: true,
                synced_at: Some(time(now_ms())),
                error,
                rate_limited_until,
            }],
            error: None,
        })
    }

    /// The open pull requests' numbers and head branches; none when they cannot be read, as the
    /// checks then only go unmatched.
    async fn open_heads(&self, project: &str) -> Vec<(u64, String)> {
        let Ok((store, machines)) = self.host_reads(project, true).await else {
            return Vec::new();
        };
        list_open_prs(&store, &machines)
            .await
            .map(|listed| {
                listed
                    .into_iter()
                    .map(|one| (one.pr.number, one.pr.head))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Run `id` with its jobs and, when it failed, the end of the failing job's log.
    pub async fn run(&self, project: &str, id: u64) -> Result<ProjectRunDetail> {
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        let place = self.run_place(&stored).map_err(FleetError::Refused)?;
        let client = self.hosts.reqwest()?;
        let calls = self.hosts.host_calls();
        let caller = place.target.caller(self.hosts.semaphore(), &client);
        let run = host_runs::one(calls, &caller, &place.target, id)
            .await
            .map_err(|e| FleetError::Refused(e.to_string()))?;
        let mut problems = Vec::new();
        let jobs = match host_runs::jobs(calls, &caller, &place.target, id).await {
            Ok(jobs) => jobs.unwrap_or_default(),
            Err(e) => {
                problems.push(e.to_string());
                Vec::new()
            }
        };
        let mut run = run;
        run.progress = run.state.going().then(|| progress(&jobs)).flatten();
        let log = match jobs.iter().find(|job| job.state == RunState::Failed {}) {
            Some(job) if run.state == (RunState::Failed {}) => {
                let step = job
                    .steps
                    .iter()
                    .find(|step| step.state == RunState::Failed {})
                    .map(|step| step.name.clone());
                match host_runs::log(calls, &caller, &place.target, job, step).await {
                    Ok(log) => Some(log),
                    Err(e) => {
                        problems.push(format!("The log did not read: {e}"));
                        None
                    }
                }
            }
            _ => None,
        };
        Ok(ProjectRunDetail {
            run,
            jobs,
            log,
            error: (!problems.is_empty()).then(|| problems.join(" ")),
            rate_limited_until: calls
                .limited_until(&place.target.who, std::time::SystemTime::now()),
        })
    }
}
