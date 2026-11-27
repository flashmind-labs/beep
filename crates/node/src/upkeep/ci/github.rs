//! Waiting on a GitHub run. `gh run watch` only redraws a screen, so its
//! exit says the run finished, a cancelled one too, and one `gh run view`
//! after it says how it ended.

use crate::process::{tail, tool_command};
use anyhow::{Context, Result, bail};
use beep_projects::repo_watch::actions::state_of;
use beep_proto::RunState;
use serde::Deserialize;
use std::{process::Stdio, time::Duration};
use tokio::{
    select,
    time::{sleep, timeout},
};

/// How long one wait on a run may take; the run is listed again after.
pub(super) const WAIT_LIMIT: Duration = Duration::from_secs(30 * 60);
/// How long one read may take.
const READ_LIMIT: Duration = Duration::from_secs(30);
/// What `gh run view` is asked for.
const FIELDS: &str = "status,conclusion,jobs,name,headBranch,url";

/// A run as `gh run view --json` gives it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub(super) struct View {
    #[serde(default)]
    status: String,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    jobs: Vec<Job>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
struct Job {
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    steps: Vec<Step>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
struct Step {
    #[serde(default)]
    name: String,
    #[serde(default)]
    conclusion: Option<String>,
}

impl View {
    pub(super) fn state(&self) -> RunState {
        state_of(&self.status, conclusion(&self.conclusion))
    }
}

/// `gh` gives an empty conclusion while a run or job goes.
fn conclusion(said: &Option<String>) -> Option<&str> {
    said.as_deref().filter(|word| !word.is_empty())
}

/// Waits until run `id` of `repo` ends: true when it passed. A run cancelled
/// meanwhile ends the wait at once. Dropping it stops `gh`.
pub(super) async fn wait(repo: &str, id: u64) -> Result<bool> {
    let id = id.to_string();
    let args: Vec<String> = [
        "run",
        "watch",
        &id,
        "-R",
        repo,
        "--exit-status",
        "--interval",
        "10",
    ]
    .map(String::from)
    .into();
    let mut watch = tool_command("gh", &args);
    watch.stdout(Stdio::null()).stderr(Stdio::null());
    let mut watch = watch.spawn().context("starting gh")?;
    let exited = select! {
        status = watch.wait() => status.context("waiting on gh")?.success(),
        () = sleep(WAIT_LIMIT) => bail!("gh waited too long"),
    };
    // The watch ended: one more read says how.
    let view = view(repo, &id).await;

    if let Ok(view) = &view {
        for change in changes(None, view) {
            log::debug!("CI watch: {repo} run {id}: {change}");
        }
    }

    match view.map(|view| view.state()) {
        Ok(RunState::Passed {}) => Ok(true),
        Ok(RunState::Failed {} | RunState::Cancelled {} | RunState::Skipped {}) => Ok(false),
        _ => Ok(exited),
    }
}

/// Run `id` of `repo` as `gh run view` reads it.
async fn view(repo: &str, id: &str) -> Result<View> {
    let args: Vec<String> = ["run", "view", id, "-R", repo, "--json", FIELDS]
        .map(String::from)
        .into();
    let mut command = tool_command("gh", &args);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = timeout(READ_LIMIT, command.output())
        .await
        .context("gh took too long")?
        .context("starting gh")?;
    if !output.status.success() {
        bail!(
            "gh failed: {}",
            tail(&String::from_utf8_lossy(&output.stderr))
        );
    }

    serde_json::from_slice(&output.stdout).context("the run does not read")
}

/// What changed from `before` to `now`: jobs that started or finished and
/// steps that failed.
pub(super) fn changes(before: Option<&View>, now: &View) -> Vec<String> {
    let mut said = Vec::new();

    for job in &now.jobs {
        let was = before.and_then(|view| view.jobs.iter().find(|old| old.name == job.name));
        let going = |job: &Job| job.status == "in_progress";
        let ended = |job: &Job| conclusion(&job.conclusion).is_some();
        if going(job) && !was.is_some_and(|was| going(was) || ended(was)) {
            said.push(format!("{} started", job.name));
        }
        if let Some(how) = conclusion(&job.conclusion)
            && !was.is_some_and(ended)
        {
            said.push(format!("{} finished: {how}", job.name));
        }

        for step in &job.steps {
            let failed = |step: &Step| conclusion(&step.conclusion) == Some("failure");
            let had = was
                .and_then(|was| was.steps.iter().find(|old| old.name == step.name))
                .is_some_and(failed);
            if failed(step) && !had {
                said.push(format!("{}: {} failed", job.name, step.name));
            }
        }
    }

    if before.is_none() && !now.name.is_empty() {
        said.insert(0, format!("{} {}", now.name, now.status));
    }
    said
}

#[cfg(test)]
#[path = "../../tests/upkeep_ci_github.rs"]
mod tests;
