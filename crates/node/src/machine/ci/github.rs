//! Waiting on a GitHub run. `gh run watch` only redraws a screen, so its
//! exit says the run finished, and `gh run view` read every 10 seconds
//! meanwhile says what changed and how it ended.

use super::forge::{WAIT_LIMIT, command, tail};
use anyhow::{Context, Result, bail};
use beep_projects::repo_watch::actions::state_of;
use beep_proto::RunState;
use serde::Deserialize;
use std::{process::Stdio, time::Duration};
use tokio::{
    select,
    time::{MissedTickBehavior, interval, sleep, timeout},
};

/// How often the run is read while the watch goes.
const EVERY: Duration = Duration::from_secs(10);
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
    let mut watch = command("gh", &args);
    watch.stdout(Stdio::null()).stderr(Stdio::null());
    let mut watch = watch.spawn().context("starting gh")?;
    let mut every = interval(EVERY);
    every.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let limit = sleep(WAIT_LIMIT);
    tokio::pin!(limit);
    let mut seen: Option<View> = None;
    let exited = loop {
        select! {
            status = watch.wait() => break status.context("waiting on gh")?.success(),
            _ = every.tick() => {
                let Ok(view) = view(repo, &id).await else { continue };
                for change in changes(seen.as_ref(), &view) {
                    log::debug!("CI watch: {repo} run {id}: {change}");
                }
                let cancelled = view.state() == RunState::Cancelled {};
                seen = Some(view);
                if cancelled {
                    return Ok(false);
                }
            }
            () = &mut limit => bail!("gh waited too long"),
        }
    };
    // The watch ended: one more read says how.
    match view(repo, &id).await.map(|view| view.state()) {
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
    let mut command = command("gh", &args);
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
mod tests {
    use super::{View, changes};
    use beep_proto::RunState;

    fn view(text: &str) -> View {
        serde_json::from_str(text).expect("the fixture reads")
    }

    #[test]
    fn only_what_changed_is_said_and_the_end_reads() {
        let first = view(
            r#"{"status":"in_progress","conclusion":"","name":"CI","headBranch":"main","url":"",
            "jobs":[{"name":"test","status":"in_progress","conclusion":"","steps":[]}]}"#,
        );
        assert_eq!(changes(None, &first), ["CI in_progress", "test started"]);
        assert!(changes(Some(&first), &first).is_empty());
        let ended = view(
            r#"{"status":"completed","conclusion":"failure","name":"CI","headBranch":"main","url":"",
            "jobs":[{"name":"test","status":"completed","conclusion":"failure",
            "steps":[{"name":"cargo test","conclusion":"failure"}]}]}"#,
        );
        assert_eq!(
            changes(Some(&first), &ended),
            ["test finished: failure", "test: cargo test failed"]
        );
        assert_eq!(first.state(), RunState::Running {});
        assert_eq!(ended.state(), RunState::Failed {});
    }
}
