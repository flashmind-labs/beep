//! Workflow runs, jobs and steps as GitHub, Gitea and Forgejo give them,
//! read into the wire's shapes, and what the page shows of them: a running
//! run's current job and step, each pull request's checks, and the end of a
//! failing job's log.

use crate::protocol::{
    PrChecks, RunJob, RunProgress, RunState, RunStep, StepProgress, WorkflowRun,
};
use chrono::{DateTime, Datelike, Utc};
use serde::Deserialize;
use std::{collections::HashSet, time::SystemTime};

/// A run list: GitHub and Gitea name it `workflow_runs`, as newer Forgejo
/// does.
#[derive(Debug, Deserialize)]
pub(crate) struct RunsReply {
    #[serde(default)]
    workflow_runs: Vec<HostRun>,
}

impl RunsReply {
    pub(crate) fn runs(self) -> Vec<WorkflowRun> {
        self.workflow_runs
            .into_iter()
            .map(HostRun::into_run)
            .collect()
    }
}

#[derive(Debug, Default, Deserialize)]
struct Login {
    #[serde(default)]
    login: String,
}

#[derive(Debug, Deserialize)]
struct PullRef {
    number: u64,
}

/// One run, with the names each host gives its fields.
#[derive(Debug, Deserialize)]
pub(crate) struct HostRun {
    id: u64,
    /// Forgejo: `index_in_repo`.
    #[serde(default, alias = "index_in_repo")]
    run_number: u64,
    /// GitHub's workflow name.
    #[serde(default)]
    name: Option<String>,
    /// The workflow file: GitHub and Gitea.
    #[serde(default)]
    path: Option<String>,
    /// Forgejo names the workflow file here; GitHub gives a number.
    #[serde(default)]
    workflow_id: Option<serde_json::Value>,
    /// Forgejo: `title`.
    #[serde(default, alias = "title")]
    display_title: String,
    #[serde(default)]
    head_branch: Option<String>,
    /// Forgejo's ref, such as `main` or `refs/pull/12/head`.
    #[serde(default)]
    prettyref: Option<String>,
    #[serde(default)]
    event: String,
    /// Forgejo: `trigger_user`.
    #[serde(default, alias = "trigger_user")]
    actor: Option<Login>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    html_url: String,
    /// Gitea: `started_at`; Forgejo: `started`.
    #[serde(default, alias = "started_at", alias = "started")]
    run_started_at: Option<String>,
    /// Gitea: `completed_at`; Forgejo: `stopped`.
    #[serde(default, alias = "stopped")]
    completed_at: Option<String>,
    /// GitHub gives no finish time: a finished run's last update is it.
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    pull_requests: Vec<PullRef>,
}

impl HostRun {
    pub(crate) fn into_run(self) -> WorkflowRun {
        let state = state_of(&self.status, self.conclusion.as_deref());
        let reference = self.prettyref.clone().unwrap_or_default();
        let branch = self
            .head_branch
            .clone()
            .filter(|branch| !branch.is_empty())
            .unwrap_or_else(|| {
                reference
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&reference)
                    .to_owned()
            });
        let mut pull_requests: Vec<u64> = self.pull_requests.iter().map(|pr| pr.number).collect();
        if let Some(number) = pull_of(&reference).or_else(|| pull_of(&branch))
            && !pull_requests.contains(&number)
        {
            pull_requests.push(number);
        }
        let finished = if state.finished() {
            time(self.completed_at.as_deref()).or_else(|| time(self.updated_at.as_deref()))
        } else {
            None
        };
        WorkflowRun {
            id: self.id,
            number: self.run_number,
            workflow: self.workflow_name(),
            title: self.display_title,
            branch,
            event: self.event,
            actor: self.actor.unwrap_or_default().login,
            state,
            url: self.html_url,
            started_at: time(self.run_started_at.as_deref()),
            finished_at: finished,
            progress: None,
            pull_requests,
        }
    }

    /// GitHub's name, else the workflow file's name without its folder or
    /// the `@ref` Gitea adds.
    fn workflow_name(&self) -> String {
        if let Some(name) = self.name.as_deref().filter(|name| !name.trim().is_empty()) {
            return name.trim().to_owned();
        }
        let file = self
            .path
            .clone()
            .or_else(|| match &self.workflow_id {
                Some(serde_json::Value::String(file)) => Some(file.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let file = file.split('@').next().unwrap_or_default();
        file.rsplit('/').next().unwrap_or_default().to_owned()
    }
}

/// The pull request `refs/pull/12/head`, `refs/pull/12/merge` or `#12`
/// names.
fn pull_of(reference: &str) -> Option<u64> {
    if let Some(rest) = reference.strip_prefix("refs/pull/") {
        return rest.split('/').next()?.parse().ok();
    }
    reference.strip_prefix('#')?.parse().ok()
}

/// A host's time, `None` when missing or Go's zero time, which Forgejo
/// sends for a run that has not started.
fn time(text: Option<&str>) -> Option<SystemTime> {
    let at = DateTime::parse_from_rfc3339(text?.trim()).ok()?;
    let at = at.with_timezone(&Utc);
    (at.year() >= 1971).then(|| at.into())
}

/// How a run, job or step stands from the host's `status` and, once it is
/// done, `conclusion`. Forgejo puts the outcome in `status` itself.
pub(crate) fn state_of(status: &str, conclusion: Option<&str>) -> RunState {
    let outcome = |word: &str| match word {
        "success" | "neutral" => Some(RunState::Passed {}),
        "failure" | "timed_out" | "startup_failure" => Some(RunState::Failed {}),
        "cancelled" => Some(RunState::Cancelled {}),
        "skipped" => Some(RunState::Skipped {}),
        "action_required" => Some(RunState::Waiting {}),
        _ => None,
    };
    let status = status.trim().to_ascii_lowercase();
    match status.as_str() {
        "queued" | "pending" | "requested" => RunState::Queued {},
        "waiting" | "blocked" => RunState::Waiting {},
        "in_progress" | "running" => RunState::Running {},
        "completed" => conclusion
            .map(str::to_ascii_lowercase)
            .and_then(|word| outcome(&word))
            .unwrap_or(RunState::Unknown {}),
        other => outcome(other).unwrap_or(RunState::Unknown {}),
    }
}

/// A run's jobs.
#[derive(Debug, Deserialize)]
pub(crate) struct JobsReply {
    #[serde(default)]
    jobs: Vec<HostJob>,
}

impl JobsReply {
    pub(crate) fn jobs(self) -> Vec<RunJob> {
        self.jobs.into_iter().map(HostJob::into_job).collect()
    }
}

#[derive(Debug, Deserialize)]
struct HostJob {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    completed_at: Option<String>,
    #[serde(default)]
    html_url: String,
    /// Missing on hosts that give jobs without steps.
    #[serde(default)]
    steps: Option<Vec<HostStep>>,
}

#[derive(Debug, Deserialize)]
struct HostStep {
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    number: Option<u32>,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    completed_at: Option<String>,
}

impl HostJob {
    fn into_job(self) -> RunJob {
        let state = state_of(&self.status, self.conclusion.as_deref());
        let steps = self
            .steps
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, step)| RunStep {
                number: step
                    .number
                    .unwrap_or_else(|| u32::try_from(i + 1).unwrap_or(u32::MAX)),
                state: state_of(&step.status, step.conclusion.as_deref()),
                name: step.name,
                started_at: time(step.started_at.as_deref()),
                finished_at: time(step.completed_at.as_deref()),
            })
            .collect();
        RunJob {
            id: self.id,
            name: self.name,
            state,
            started_at: time(self.started_at.as_deref()),
            finished_at: if state.finished() {
                time(self.completed_at.as_deref())
            } else {
                None
            },
            steps,
            url: self.html_url,
        }
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Where a run with `jobs` is: the running job, else the next one queued,
/// and its running step, else the first one not done. `None` when no job
/// is left to run.
pub(crate) fn progress(jobs: &[RunJob]) -> Option<RunProgress> {
    let current = jobs
        .iter()
        .find(|job| job.state == RunState::Running {})
        .or_else(|| jobs.iter().find(|job| job.state.going()))?;
    let step = current
        .steps
        .iter()
        .find(|step| step.state == RunState::Running {})
        .or_else(|| current.steps.iter().find(|step| !step.state.finished()))
        .map(|step| StepProgress {
            number: step.number,
            of: count(current.steps.len()),
            name: step.name.clone(),
            started_at: step.started_at,
        });
    Some(RunProgress {
        job: current.name.clone(),
        job_started_at: current.started_at,
        jobs_done: count(jobs.iter().filter(|job| job.state.finished()).count()),
        jobs: count(jobs.len()),
        step,
    })
}

/// An open pull request as checks match it: its number and head branch.
pub(crate) struct PrHeadRef<'a> {
    pub(crate) number: u64,
    /// Empty when unknown.
    pub(crate) head: &'a str,
}

/// How each pull request's newest run of each workflow stands. A run is a
/// pull request's when the host names it, or when it ran for a pull
/// request event on its head branch. `runs` are newest first.
pub(crate) fn pr_checks(runs: &[WorkflowRun], prs: &[PrHeadRef<'_>]) -> Vec<PrChecks> {
    prs.iter()
        .filter_map(|pr| {
            let mut workflows = HashSet::new();
            let newest: Vec<&WorkflowRun> = runs
                .iter()
                .filter(|run| {
                    run.pull_requests.contains(&pr.number)
                        || (!pr.head.is_empty()
                            && run.branch == pr.head
                            && run.event.starts_with("pull_request"))
                })
                .filter(|run| workflows.insert(run.workflow.as_str()))
                .collect();
            let decided = newest
                .iter()
                .find(|run| run.state == RunState::Failed {})
                .or_else(|| newest.iter().find(|run| run.state.going()))
                .or_else(|| newest.iter().find(|run| run.state == RunState::Passed {}))
                .or_else(|| newest.first())?;
            Some(PrChecks {
                number: pr.number,
                state: decided.state,
                run_id: decided.id,
                progress: decided.progress.clone(),
            })
        })
        .collect()
}

/// The lines of a log kept for a failed job.
pub(crate) const LOG_LINES: usize = 120;

/// The part of a failed job's log worth showing, at most `max` lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogCut {
    pub(crate) text: String,
    /// The step the log names, from GitHub's `##[group]` line.
    pub(crate) step: Option<String>,
    pub(crate) truncated: bool,
}

/// From GitHub's log, the failing step's lines: from the last
/// `##[group]` before the last `##[error]` up to just after it. Without
/// those marks, the last lines. Timestamps and terminal colours go.
pub(crate) fn failing_part(log: &str, max: usize) -> LogCut {
    let lines: Vec<String> = log.lines().map(clean_line).collect();
    let error = lines.iter().rposition(|line| line.contains("##[error]"));
    let (start, end, step) = match error {
        Some(error) => {
            let group = lines[..error]
                .iter()
                .rposition(|line| line.starts_with("##[group]"));
            let step = group.map(|at| lines[at].trim_start_matches("##[group]").trim().to_owned());
            (group.unwrap_or(0), (error + 4).min(lines.len()), step)
        }
        None => (0, lines.len(), None),
    };
    let part = &lines[start..end];
    let from = part.len().saturating_sub(max);
    LogCut {
        text: part[from..].join("\n"),
        step,
        truncated: start > 0 || from > 0,
    }
}

/// A log line without its leading timestamp and without escape codes.
fn clean_line(line: &str) -> String {
    let line = line.trim_end_matches('\r');
    let stamped = line.len() > 20
        && line.as_bytes()[..4].iter().all(u8::is_ascii_digit)
        && line.as_bytes()[4] == b'-'
        && line.as_bytes()[10] == b'T';
    let line = if stamped {
        line.split_once(' ').map_or(line, |(_, rest)| rest)
    } else {
        line
    };
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests;
