//! A project's CI runs, read from its hosts' Actions API with the user's token: the list with each
//! running run's progress, how each open pull request's checks stand, and one run's jobs with the
//! end of a failed job's log. A repository pushed to GitHub and a Forgejo server runs on both, so
//! every host a checkout's remotes name is read too. The reads and their ETag cache are
//! `beep_projects::repo_watch`'s.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    records::{Stored, find_project, now_ms, time},
};
use beep_projects::{
    branches::{READ_LIMIT, run_script},
    credentials::Opened,
    git_items::HostError,
    imports::{
        hosts::{credential_of, forgejo_of},
        preview::{parse_remotes, remotes_script},
        repo::{Host, RepoRef},
    },
    model::RepoName,
    repo_watch::{
        actions::{PrHeadRef, RunsReply, pr_checks, progress},
        runs::{self as host_runs, Target},
    },
};
use beep_proto::{
    GitHostKind, HostedChecks, HostedRun, HostedRuns, ProjectRunDetail, RunState, RunsHost,
    Service, WorkflowRun,
};
use futures::{
    StreamExt,
    future::{join, join_all},
    stream,
};
use secrecy::ExposeSecret;
use std::str::FromStr;
use std::time::{Duration, SystemTime};

/// What the host's rate limit is shared by: the desktop's calls to it.
const WHO: &str = "desktop";
/// The most running runs whose jobs are read for their progress.
const PROGRESS_READS: usize = 5;
/// The most hosts one project's runs are read from.
const MAX_HOSTS: usize = 4;
/// How long a checkout's remotes are kept.
const REMOTES_FOR: Duration = Duration::from_secs(30 * 60);
/// The most machines asked for a checkout's remotes.
const REMOTES_ASKED: usize = 2;
const NO_HOST: &str =
    "This project's repository is not on a GitHub or Forgejo host Beep has a token for.";

/// Where a project's runs are read.
struct Place {
    target: Target,
    kind: GitHostKind,
    repo: String,
    /// The repository's web page on the host.
    web: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteScope {
    Nearest,
    All,
}

impl Desk {
    /// The project's own host and the token that reads it, or why there is none.
    fn run_place(&self, stored: &Stored) -> std::result::Result<Place, String> {
        let opened = self.opened();
        let repo = RepoRef::parse(&stored.record.url, &forgejo_of(&opened))
            .map_err(|_| NO_HOST.to_owned())?;
        self.place_of(&repo, &opened)
    }

    /// Where `repo`'s runs are read, with the user's token for its host.
    fn place_of(&self, repo: &RepoRef, opened: &[Opened]) -> std::result::Result<Place, String> {
        let found = credential_of(repo, opened)
            .ok_or_else(|| format!("Beep has no token for {}.", repo.host_name()))?;
        let name = RepoName::from_str(&repo.project()).map_err(|e| e.to_string())?;
        let secret = found.secret.expose_secret().to_owned().into();
        let (mut target, kind) = match (&repo.host, &found.service) {
            (Host::GitHub, Service::GitHub) => {
                let account = found
                    .github
                    .as_ref()
                    .map_or("", |login| login.login.as_str());
                (
                    Target::github(WHO, name, secret, account),
                    GitHostKind::GitHub {},
                )
            }
            (Host::Forgejo(url), Service::Forgejo { .. }) => (
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
            web: repo.web_url(),
        })
    }

    /// Every host the project's repository is on with a token here: its own first, then each
    /// host a checkout's remote names it on (by full name, or by name under another owner). Two
    /// remotes to one server, such as over ssh and over http, are one host.
    async fn run_places(&self, stored: &Stored) -> std::result::Result<Vec<Place>, String> {
        let opened = self.opened();
        let servers = forgejo_of(&opened);
        let own = RepoRef::parse(&stored.record.url, &servers).map_err(|_| NO_HOST.to_owned())?;
        let remotes: Vec<RepoRef> = self
            .checkout_remotes(stored, RemoteScope::Nearest)
            .await
            .iter()
            .filter_map(|url| RepoRef::parse(url, &servers).ok())
            .collect();
        let full = remotes
            .iter()
            .filter(|repo| repo.project().eq_ignore_ascii_case(&own.project()));
        let named = remotes.iter().filter(|repo| {
            repo.name().eq_ignore_ascii_case(own.name())
                || stored
                    .record
                    .remote_renames
                    .iter()
                    .any(|rename| rename.to == repo.web_url())
        });
        let mut repos: Vec<&RepoRef> = Vec::new();

        for repo in [&own].into_iter().chain(full).chain(named) {
            if !repos.iter().any(|seen| seen.host == repo.host) {
                repos.push(repo);
            }
        }

        let mut places: Vec<Place> = Vec::new();
        let mut first_error = None;

        for repo in repos {
            match self.place_of(repo, &opened) {
                Ok(place) if !places.iter().any(|p| p.target.host == place.target.host) => {
                    places.push(place);
                }
                Ok(_) => {}
                Err(why) => {
                    first_error.get_or_insert(why);
                }
            }
        }

        places.truncate(MAX_HOSTS);

        match first_error {
            Some(why) if places.is_empty() => Err(why),
            _ => Ok(places),
        }
    }

    /// Cached remotes for live views, or all checkouts for explicit repository discovery.
    /// Saved renames apply even before an offline machine has updated its Git config.
    pub(crate) async fn checkout_remotes(
        &self,
        stored: &Stored,
        scope: RemoteScope,
    ) -> Vec<String> {
        let current = |urls: Vec<String>| {
            urls.into_iter()
                .map(|mut url| {
                    for rename in &stored.record.remote_renames {
                        if let Some(next) =
                            beep_projects::imports::repo::renamed_remote(&url, rename)
                        {
                            url = next;
                        }
                    }
                    url
                })
                .collect()
        };
        let cached = self.hosts.remotes(&stored.name, REMOTES_FOR);
        if scope == RemoteScope::Nearest
            && let Some(kept) = cached.as_ref()
        {
            return current(kept.clone());
        }

        let mut machines: Vec<_> = stored.places.keys().copied().collect();
        crate::nearest_first(&self.mesh, &mut machines, |machine| *machine);
        machines.sort_by_key(|machine| *machine != self.local);
        if scope == RemoteScope::Nearest {
            machines.truncate(REMOTES_ASKED);
        }
        // Explicit discovery reads all checkouts, including machine-specific remotes.
        let mut reads = stream::iter(machines)
            .map(|id| async move {
                let seen = self.seen_one(id).await?;
                let shell = seen.machine.shell().ok()?;
                let folder = stored.places[&id].folder.clone();
                match run_script(shell, &remotes_script(&[folder]), READ_LIMIT).await {
                    Ok(out) => Some(
                        parse_remotes(&out)
                            .into_iter()
                            .flat_map(|folder| folder.remotes.into_iter().map(|(_, url)| url))
                            .collect::<Vec<_>>(),
                    ),
                    Err(error) => {
                        log::info!("repository remotes could not be read: {error:#}");
                        None
                    }
                }
            })
            .buffered(if scope == RemoteScope::All { 4 } else { 1 });
        let mut found = if scope == RemoteScope::All {
            cached.unwrap_or_default()
        } else {
            Vec::new()
        };
        while let Some(urls) = reads.next().await {
            if let Some(urls) = urls {
                for url in urls {
                    if !found.contains(&url) {
                        found.push(url);
                    }
                }
                if scope == RemoteScope::Nearest {
                    break;
                }
            }
        }
        if !found.is_empty() {
            self.hosts.keep_remotes(&stored.name, found.clone());
        }
        current(found)
    }

    /// The newest runs of the project from every host it is on, newest first, and the checks of
    /// its open pull requests on each.
    pub async fn runs(&self, project: &str) -> Result<HostedRuns> {
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        let places = match self.run_places(&stored).await {
            Ok(places) => places,
            Err(why) => {
                return Ok(HostedRuns {
                    runs: Vec::new(),
                    checks: Vec::new(),
                    hosts: Vec::new(),
                    error: Some(why),
                });
            }
        };
        // The hosts and the open pull requests are asked at once.
        let (read, heads) = join(
            join_all(places.iter().map(|place| self.host_runs(place))),
            self.open_heads(&places[0]),
        )
        .await;
        let refs: Vec<PrHeadRef<'_>> = heads
            .iter()
            .map(|(number, head)| PrHeadRef {
                number: *number,
                head,
            })
            .collect();
        let mut all = HostedRuns {
            runs: Vec::new(),
            checks: Vec::new(),
            hosts: Vec::new(),
            error: None,
        };

        for (runs, mut host) in read {
            host.primary = all.hosts.is_empty();
            let checks = pr_checks(&runs, &refs);
            all.checks
                .extend(checks.into_iter().map(|checks| HostedChecks {
                    host: host.host.clone(),
                    checks,
                }));
            all.runs.extend(runs.into_iter().map(|run| HostedRun {
                host: host.host.clone(),
                run,
            }));
            all.hosts.push(host);
        }

        newest_first(&mut all.runs);
        Ok(all)
    }

    async fn fallback_runs(&self, host: &RunsHost) -> Option<Vec<WorkflowRun>> {
        let script = fallback_script(host)?;
        let mut machines: Vec<_> = self
            .mesh
            .code_hosts()
            .into_iter()
            .filter(|status| status.online && status.device.id != self.local)
            .map(|status| status.device.id)
            .collect();
        crate::nearest_first(&self.mesh, &mut machines, |id| *id);

        for id in machines.into_iter().take(2) {
            let Some(seen) = self.seen_one(id).await else {
                continue;
            };
            let Ok(shell) = seen.machine.shell() else {
                continue;
            };
            if let Ok(output) = run_script(shell, &script, READ_LIMIT).await
                && let Ok(reply) = serde_json::from_str::<RunsReply>(&output)
            {
                return Some(reply.runs());
            }
        }

        None
    }

    /// One host's newest runs, with the progress of those running, and how its read went.
    async fn host_runs(&self, place: &Place) -> (Vec<WorkflowRun>, RunsHost) {
        let calls = &self.hosts.kept;
        let mut host = RunsHost {
            host: place.target.host.clone(),
            kind: place.kind,
            repo: place.repo.clone(),
            primary: false,
            synced_at: Some(time(now_ms())),
            error: None,
            rate_limited_until: None,
            url: Some(place.web.clone()),
        };
        let caller = place.target.caller(&self.hosts.calls, &self.hosts.client);
        let mut shown = match host_runs::list(calls, &caller, &place.target).await {
            Ok(runs) => runs,
            Err(e) => {
                if matches!(e, HostError::Timeout(_) | HostError::Unreachable(_))
                    && let Some(runs) = self.fallback_runs(&host).await
                {
                    return (runs, host);
                }

                host.error = Some(e.to_string());
                Vec::new()
            }
        };
        // The running runs' jobs are read together, the host's calls bounded by the caller's
        // semaphore; none while the rate limit is low, as they are only for live progress.
        let low = calls
            .limited_until(&place.target.who, SystemTime::now())
            .is_some();
        if !low {
            let going = shown
                .iter_mut()
                .filter(|run| run.state.going())
                .take(PROGRESS_READS);
            join_all(going.map(|run| async {
                if let Ok(Some(jobs)) = host_runs::jobs(calls, &caller, &place.target, run.id).await
                {
                    run.progress = progress(&jobs);
                }
            }))
            .await;
        }

        host.rate_limited_until = calls.limited_until(&place.target.who, SystemTime::now());
        (shown, host)
    }

    /// The open pull requests' numbers and head branches; none when they cannot be read, as the
    /// checks then only go unmatched.
    async fn open_heads(&self, place: &Place) -> Vec<(u64, String)> {
        let caller = place.target.caller(&self.hosts.calls, &self.hosts.client);
        host_runs::heads(&self.hosts.kept, &caller, &place.target)
            .await
            .unwrap_or_default()
    }

    /// Run `id` on `host` (the project's own host for `None`) with its jobs and, when it failed,
    /// the end of the failing job's log.
    pub async fn run(
        &self,
        project: &str,
        host: Option<&str>,
        id: u64,
    ) -> Result<ProjectRunDetail> {
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        let own = self.run_place(&stored);
        let elsewhere = host.filter(|host| own.as_ref().is_ok_and(|own| own.target.host != *host));
        let other = match elsewhere {
            Some(host) => self
                .run_places(&stored)
                .await
                .ok()
                .and_then(|places| places.into_iter().find(|place| place.target.host == host)),
            None => None,
        };
        let place = match other {
            Some(place) => place,
            None => own.map_err(FleetError::Refused)?,
        };
        let calls = &self.hosts.kept;
        let caller = place.target.caller(&self.hosts.calls, &self.hosts.client);
        // The run and its jobs are asked at once; the log waits for the jobs, which name it.
        let (run, jobs) = join(
            host_runs::one(calls, &caller, &place.target, id),
            host_runs::jobs(calls, &caller, &place.target, id),
        )
        .await;
        let mut run = run.map_err(|e| FleetError::Refused(e.to_string()))?;
        let mut problems = Vec::new();
        let jobs = match jobs {
            Ok(jobs) => jobs.unwrap_or_default(),
            Err(e) => {
                problems.push(e.to_string());
                Vec::new()
            }
        };
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
            rate_limited_until: calls.limited_until(&place.target.who, SystemTime::now()),
        })
    }
}

/// Sorts runs newest first by start, or by finish for one cancelled before it started, with
/// those still waiting to start first; ties keep their order.
fn newest_first(runs: &mut [HostedRun]) {
    runs.sort_by(|a, b| {
        let key = |r: &HostedRun| {
            let at = r.run.started_at.or(r.run.finished_at);
            (at.is_none(), at)
        };
        key(b).cmp(&key(a))
    });
}

/// Node CLIs use credentials provisioned from the mesh; no token enters the script.
fn fallback_script(host: &RunsHost) -> Option<String> {
    let path = format!(
        "repos/{}/actions/runs?per_page=20&limit=20&page=1",
        host.repo
    );
    let path = beep_projects::quote(&path);
    match host.kind {
        GitHostKind::GitHub {} => Some(format!(
            "{}gh api {path}",
            beep_projects::commands::gh_account(&host.repo)
        )),
        GitHostKind::Forgejo {} => {
            let web = host.url.as_ref()?;
            let suffix = format!("/{}", host.repo);
            let base = web.strip_suffix(&suffix)?;
            Some(format!(
                "teacli api {path} --host {}",
                beep_projects::quote(base)
            ))
        }
    }
}
