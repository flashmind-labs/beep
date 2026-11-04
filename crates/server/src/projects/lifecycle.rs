//! Running project workers: their reports, re-attaching, PR checks, idle close and queued starts.

use super::{
    commands::{
        HostCommand, LABEL_BLOCKED, LABEL_REVIEW, LABEL_RUNNING, PrState, parse_pr, pr_fingerprint,
        worktree_remove,
    },
    fleet::{self, Fleet, NodeFleet},
    gate::ProjectGate,
    issues::run_host,
    model::{
        CHECK_EVERY, IDLE_CLOSE, IssueHost, ProjectId, TaskState, WorkerId, WorkerState, pr_number,
    },
    policy::{Verdict, WorkerScope},
    schedule::Due,
    turn::{CHECK_LABEL, REPLAN_LABEL, WORKER_LABEL},
    worker::Started,
};
use crate::{
    account::user::User,
    api::transport::READ_TIMEOUT,
    app::App,
    nodes::{News, SessionRefused},
    protocol::node::agent::{OptionKind, Question, Request as AgentRequest, SessionInfo, Status},
    store::{ProjectRow, StartRefused, TaskRow, WorkerRow},
};
use anyhow::{Context, Result};
use chrono::{DateTime, TimeDelta, Utc};
use futures::StreamExt;
use std::{fmt::Write, sync::Arc, time::Duration};
use tokio::time::timeout;
use uuid::Uuid;

/// Workers re-attached at once after a reconnect.
const REATTACH_AT_ONCE: usize = 4;
/// Most pull requests one check reads, and how long each may take.
const MAX_CHECKED: usize = 8;
const PR_LIMIT: Duration = Duration::from_secs(10);
/// Most queued tasks one pass tries to start.
const MAX_FILLS: usize = 6;
/// The longest last message a report quotes.
const MAX_QUOTED: usize = 4000;

fn after(at: DateTime<Utc>, wait: Duration) -> DateTime<Utc> {
    at + TimeDelta::from_std(wait).unwrap_or(TimeDelta::MAX)
}

/// The owner of `worker` and a fleet of their nodes, while signed in.
async fn fleet_of_worker(app: &Arc<App>, worker: WorkerId) -> Result<Option<NodeFleet>> {
    let Some(owner) = app.store.worker_owner(worker).await? else {
        return Ok(None);
    };
    Ok(fleet_of(app, &owner))
}

fn fleet_of(app: &Arc<App>, owner: &str) -> Option<NodeFleet> {
    Some(NodeFleet::new(app, &app.user(owner)?))
}

/// Handles a report from `worker`'s session; `waited` means a tool call already told the chat.
pub(crate) async fn on_news(
    app: &Arc<App>,
    worker: WorkerId,
    news: News,
    waited: bool,
) -> Result<()> {
    let Some(fleet) = fleet_of_worker(app, worker).await? else {
        return Ok(());
    };
    let user = fleet.user.clone();
    match news {
        News::TurnDone { turn, last_message } => {
            let done = TurnDone {
                turn,
                last_message: &last_message,
                report: !waited,
            };
            turn_done(&fleet, &user, worker, &done, Utc::now()).await
        }
        News::Question(question) => asked(&fleet, &user, worker, question, !waited).await,
        News::Ended { reason } => ended(&fleet, &user, worker, &reason).await,
    }
}

/// Catches up on what `worker`'s session did before its reports reached the project.
pub(crate) async fn caught_up(app: &Arc<App>, worker: WorkerId, info: SessionInfo) -> Result<()> {
    let Some(fleet) = fleet_of_worker(app, worker).await? else {
        return Ok(());
    };
    let user = fleet.user.clone();
    catch_up(&fleet, &user, worker, info).await
}

async fn catch_up(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    worker: WorkerId,
    info: SessionInfo,
) -> Result<()> {
    if info.state.status == Status::Ended {
        return ended(fleet, user, worker, "it ended while nobody followed it").await;
    }
    if info.turns > 0 {
        let done = TurnDone {
            turn: info.turns,
            last_message: &info.last_message,
            report: true,
        };
        turn_done(fleet, user, worker, &done, Utc::now()).await?;
    }
    if let Some(question) = info.question {
        asked(fleet, user, worker, question, true).await?;
    }
    Ok(())
}

/// A worker with what it works on.
struct Working {
    worker: WorkerRow,
    task: TaskRow,
    project: ProjectRow,
    /// The project's chat.
    chat: Uuid,
    /// The worker's machine, as commands name it.
    machine: String,
}

async fn working(fleet: &dyn Fleet, user: &User, worker: WorkerId) -> Result<Option<Working>> {
    let Some(worker) = user.project_worker(worker).await? else {
        return Ok(None);
    };
    let Some(task) = user.project_task(worker.task).await? else {
        return Ok(None);
    };
    let Some(project) = user.project(worker.project).await? else {
        return Ok(None);
    };
    let chat = user.ensure_project_agent(project.id).await?;
    let machines = fleet.machines().await?;
    let machine = fleet::name_of(&machines, &worker.machine);
    Ok(Some(Working {
        worker,
        task,
        project,
        chat,
        machine,
    }))
}

/// A finished turn.
pub(crate) struct TurnDone<'a> {
    pub(crate) turn: u32,
    pub(crate) last_message: &'a str,
    /// Whether to tell the chat; not when a tool call waited for it.
    pub(crate) report: bool,
}

/// A turn ended: notes it, arms the idle close, looks for the task's PR and tells the chat.
pub(crate) async fn turn_done(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    worker: WorkerId,
    done: &TurnDone<'_>,
    now: DateTime<Utc>,
) -> Result<()> {
    let Some(Working {
        worker,
        task,
        project,
        chat,
        machine,
    }) = working(fleet, user, worker).await?
    else {
        return Ok(());
    };
    if !worker.state.is_live()
        || !fleet
            .projects()
            .take_turn(worker.id, done.turn, worker.turns_seen)
    {
        return Ok(());
    }
    // Stale: a newer turn was noted since this one was read.
    if !user
        .worker_turn(worker.id, done.turn, done.last_message, now)
        .await?
    {
        return Ok(());
    }
    fleet
        .projects()
        .arm(Due::Idle(worker.id), after(now, IDLE_CLOSE));
    let mut pr_line = "No pull request yet.".to_owned();
    let mut leaks = Vec::new();
    if let Some(prs) = &project.roles.prs {
        match find_pr(fleet, user, &project, prs, &task).await {
            Ok(Some(pr)) => {
                pr_line = format!("Pull request: {}", pr.url);
                // Only from running as the store has it now: the worker may
                // have ended, and its task blocked, while this looked.
                if task.state == TaskState::Running
                    && user
                        .move_task_state(task.id, TaskState::Running, TaskState::Review)
                        .await?
                {
                    user.set_task_pr(task.id, &pr.url).await?;
                    user.set_pr_seen(task.id, &pr_fingerprint(&pr)).await?;
                    relabel(
                        fleet,
                        user,
                        &project,
                        &task,
                        Some(LABEL_REVIEW),
                        &[LABEL_RUNNING],
                    )
                    .await;
                    fleet
                        .projects()
                        .arm(Due::Check(project.id), after(now, CHECK_EVERY));
                }
                let machines = fleet.machines().await?;
                let public_text =
                    fleet::project_public_text(fleet, user, &project, &machines).await?;
                for hit in [&pr.title, &pr.body]
                    .into_iter()
                    .flat_map(|text| public_text.hits(text))
                {
                    if !leaks.contains(&hit) {
                        leaks.push(hit);
                    }
                }
            }
            Ok(None) => {}
            Err(why) => pr_line = format!("Could not look for its pull request: {why:#}"),
        }
    }
    if !done.report {
        return Ok(());
    }
    let mut text = format!(
        "The worker for {} ({}) on {machine} finished turn {}.",
        task.slug, task.title, done.turn
    );
    let last = done.last_message.trim();
    if !last.is_empty() {
        let _ = write!(text, " Its last message:\n{}", quote(last));
    }
    let _ = write!(text, "\n\n{pr_line}");
    if !leaks.is_empty() {
        let _ = write!(
            text,
            "\nThe pull request names private details: {}. Have the worker remove them.",
            leaks.join(", ")
        );
    }
    let _ = write!(
        text,
        "\n\nTell the user in a few sentences what it did. project_worker with action prompt \
         and task {} gives it more to do.",
        task.slug
    );
    fleet.post(chat, WORKER_LABEL, text);
    Ok(())
}

/// The open pull request from the task's branch, read on the home machine.
async fn find_pr(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    prs: &IssueHost,
    task: &TaskRow,
) -> Result<Option<PrState>> {
    let home = fleet::home_shell(fleet, user, project)
        .await
        .map_err(anyhow::Error::msg)?;
    let branch = task.branch.clone().unwrap_or_else(|| task.slug.branch());
    let command = HostCommand::FindPr {
        branch: &branch,
        base: &project.default_branch,
    };
    let out = run_host(home.as_ref(), prs, project, command).await?;
    parse_pr(prs, &out)
}

/// Moves the task's issue labels; a failure only costs the label.
async fn relabel(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    task: &TaskRow,
    add: Option<&str>,
    remove: &[&str],
) {
    let (Some(issue), Some(host)) = (&task.issue, &task.issue_host) else {
        return;
    };
    // Someone else's issue keeps its labels.
    if task.issue_foreign {
        return;
    }
    let home = match fleet::home_shell(fleet, user, project).await {
        Ok(home) => home,
        Err(why) => {
            log::info!("projects: an issue label waits: {why}");
            return;
        }
    };
    let command = HostCommand::Relabel {
        number: issue.number,
        add,
        remove,
    };
    if let Err(error) = run_host(home.as_ref(), host, project, command).await {
        log::warn!("projects: unable to relabel an issue: {error:#}");
    }
}

/// Blocks the task if it is still running, labelling its issue.
pub(crate) async fn block(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    task: &TaskRow,
) -> Result<()> {
    if task.state == TaskState::Running
        && user
            .move_task_state(task.id, TaskState::Running, TaskState::Blocked)
            .await?
    {
        relabel(
            fleet,
            user,
            project,
            task,
            Some(LABEL_BLOCKED),
            &[LABEL_RUNNING],
        )
        .await;
    }
    Ok(())
}

/// The id of the first option of `kinds` the question offers.
fn option_of(question: &Question, kinds: &[OptionKind]) -> Option<String> {
    kinds.iter().find_map(|kind| {
        question
            .options
            .iter()
            .find(|option| option.kind == *kind)
            .map(|option| option.id.clone())
    })
}

/// The worker asks something: the policy answers it, else the user does.
pub(crate) async fn asked(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    worker: WorkerId,
    question: Question,
    report: bool,
) -> Result<()> {
    let Some(working) = working(fleet, user, worker).await? else {
        return Ok(());
    };
    let worker = &working.worker;
    if !worker.state.is_live() || !fleet.projects().take_question(worker.id, question.id) {
        return Ok(());
    }
    let (worker_id, question_id) = (worker.id, question.id);
    let handled = handle_question(fleet, user, working, question, report).await;
    // One that was not handled is handled again when it next shows up, as
    // on a catch-up, instead of being dropped.
    if handled.is_err() {
        fleet.projects().forget_question(worker_id, question_id);
    }
    handled
}

/// Answers `question` by the policy when it can, else puts it to the user.
async fn handle_question(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    working: Working,
    question: Question,
    report: bool,
) -> Result<()> {
    let Working {
        worker,
        task,
        project,
        chat,
        machine,
    } = working;
    let gate = ProjectGate::new(user.app_rules().await?, None, project.visibility);
    let machines = fleet.machines().await?;
    let public_text = fleet::project_public_text(fleet, user, &project, &machines).await?;
    let branch = task.branch.clone().unwrap_or_else(|| task.slug.branch());
    let scope = WorkerScope {
        repo: &project.name,
        worktree: &worker.folder,
        branch: &branch,
        default_branch: &project.default_branch,
        visibility: project.visibility,
        outside_allowed: gate.outside_allowed(),
        public_text: &public_text,
        build_edited: false,
    };
    let verdict = fleet.projects().judge(worker.id, &question, &scope);
    let allow = option_of(&question, &[OptionKind::AllowOnce, OptionKind::AllowAlways]);
    match verdict {
        Verdict::Allow if allow.is_some() => {
            let reply = allow.context("an allow option")?;
            fleet
                .send_session(
                    &worker,
                    AgentRequest::Answer {
                        question: question.id,
                        reply,
                    },
                )
                .await?;
            fleet.projects().allowed(worker.id, &question);
            Ok(())
        }
        Verdict::Refuse(reason) => {
            if let Some(reply) = option_of(
                &question,
                &[OptionKind::RejectOnce, OptionKind::RejectAlways],
            ) {
                fleet
                    .send_session(
                        &worker,
                        AgentRequest::Answer {
                            question: question.id,
                            reply,
                        },
                    )
                    .await?;
            }
            fleet
                .send_session(
                    &worker,
                    AgentRequest::Prompt {
                        text: reason.clone(),
                    },
                )
                .await?;
            if report {
                fleet.post(
                    chat,
                    WORKER_LABEL,
                    format!(
                        "The worker for {} on {machine} asked: {}. It was refused and told: \
                         {reason} Tell the user only if it matters.",
                        task.slug, question.text
                    ),
                );
            }
            Ok(())
        }
        // An allow the agent cannot be given goes to the user too.
        Verdict::Allow | Verdict::AskUser => {
            user.set_worker_asking(worker.id, true).await?;
            if report {
                let mut text = format!(
                    "The worker for {} on {machine} asks: {}",
                    task.slug, question.text
                );
                if let Some(detail) = &question.detail {
                    let _ = write!(text, " ({detail})");
                }
                let _ = write!(
                    text,
                    "\nThe user has a card to answer it. You can answer instead with \
                     project_worker, action answer, task {} and the option as prompt. Tell the \
                     user briefly what it waits for.",
                    task.slug
                );
                fleet.post(chat, WORKER_LABEL, text);
            }
            fleet.ask_user(&worker, &task, chat, question).await
        }
    }
}

/// The worker's session ended: the worker ends, a running task blocks, the agent re-plans.
pub(crate) async fn ended(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    worker: WorkerId,
    reason: &str,
) -> Result<()> {
    let Some(Working {
        worker,
        task,
        project,
        chat,
        machine,
    }) = working(fleet, user, worker).await?
    else {
        return Ok(());
    };
    fleet.projects().untrack(worker.id);
    // One stopped or closed already was handled where that happened.
    if !worker.state.is_live() {
        return Ok(());
    }
    user.set_worker_state(worker.id, WorkerState::Ended).await?;
    block(fleet, user, &project, &task).await?;
    let now = if task.state == TaskState::Running {
        "blocked"
    } else {
        task_state_name(task.state)
    };
    fleet.post(
        chat,
        REPLAN_LABEL,
        format!(
            "The worker for {} on {machine} ended: {reason}. The task is {now}. Decide what \
             comes next: start it again, change the task, or tell the user.",
            task.slug
        ),
    );
    fill_slots(fleet, user).await
}

fn task_state_name(state: TaskState) -> &'static str {
    state.into()
}

/// `machine` reconnected: re-attaches its live workers, failing those whose session is gone.
pub(crate) async fn reconnected(
    app: &Arc<App>,
    user: &Arc<User>,
    machine: iroh::EndpointId,
) -> Result<()> {
    let fleet = NodeFleet::new(app, user);
    reattach_all(&fleet, user, machine).await
}

pub(crate) async fn reattach_all(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    machine: iroh::EndpointId,
) -> Result<()> {
    let workers = user.live_workers_on(&machine).await?;
    futures::stream::iter(workers)
        .for_each_concurrent(REATTACH_AT_ONCE, |worker| async move {
            if let Err(error) = reattach(fleet, user, &worker).await {
                log::warn!("projects: unable to re-attach a worker: {error:#}");
            }
        })
        .await;
    Ok(())
}

async fn reattach(fleet: &dyn Fleet, user: &Arc<User>, worker: &WorkerRow) -> Result<()> {
    // One still starting gets its session from the start under way.
    let Some(session) = &worker.session else {
        return Ok(());
    };
    let chat = user.ensure_project_agent(worker.project).await?;
    // Tracked first, so what it reports from now on reaches the project.
    fleet.projects().track(worker.machine, session, worker.id);
    match timeout(READ_TIMEOUT, fleet.attach(worker, chat)).await {
        Ok(Ok(info)) => catch_up(fleet, user, worker.id, info).await,
        Ok(Err(why)) if why.downcast_ref::<SessionRefused>().is_some() => {
            log::info!("projects: a worker's session is gone: {why:#}");
            gone(fleet, user, worker.id).await
        }
        // A stream that would not open or closed early, as a machine that
        // flaps does: the worker stays as it was and the next reconnect tries
        // again.
        Ok(Err(why)) => {
            log::warn!("projects: unable to re-attach a worker; it stays as it was: {why:#}");
            Ok(())
        }
        Err(_) => {
            log::warn!("projects: the machine did not answer for a worker; it stays as it was");
            Ok(())
        }
    }
}

/// The machine no longer has the worker's session.
async fn gone(fleet: &dyn Fleet, user: &Arc<User>, worker: WorkerId) -> Result<()> {
    let Some(Working {
        worker,
        task,
        project,
        chat,
        machine,
    }) = working(fleet, user, worker).await?
    else {
        return Ok(());
    };
    fleet.projects().untrack(worker.id);
    if !worker.state.is_live() {
        return Ok(());
    }
    user.set_worker_state(worker.id, WorkerState::Failed)
        .await?;
    block(fleet, user, &project, &task).await?;
    fleet.post(
        chat,
        REPLAN_LABEL,
        format!(
            "The worker for {} on {machine} is gone; the machine no longer has its session.",
            task.slug
        ),
    );
    Ok(())
}

/// Something on the schedule is due.
pub(crate) async fn on_due(app: &Arc<App>, due: Due, now: DateTime<Utc>) -> Result<()> {
    match due {
        Due::Check(project) => {
            let Some(owner) = app.store.project_owner(project).await? else {
                return Ok(());
            };
            let Some(fleet) = fleet_of(app, &owner) else {
                return Ok(());
            };
            let user = fleet.user.clone();
            check(&fleet, &user, project, now).await
        }
        Due::Idle(worker) => {
            let Some(fleet) = fleet_of_worker(app, worker).await? else {
                return Ok(());
            };
            let user = fleet.user.clone();
            idle_close(&fleet, &user, worker, now).await
        }
        // The projects run a worker's jobs themselves.
        Due::Worker(_) => Ok(()),
    }
}

/// Checks PRs of tasks in review, finishes merged ones, tells the chat, re-arms while needed.
pub(crate) async fn check(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    project: ProjectId,
    now: DateTime<Utc>,
) -> Result<()> {
    let Some(project) = user.project(project).await? else {
        fleet.projects().disarm(&Due::Check(project));
        return Ok(());
    };
    // A failure on one pull request must not stop the checks for good.
    let checked = check_prs(fleet, user, &project).await;
    rearm_check(fleet, user, project.id, now).await?;
    checked
}

/// [`check`]'s reading of the project's pull requests.
async fn check_prs(fleet: &dyn Fleet, user: &Arc<User>, project: &ProjectRow) -> Result<()> {
    let mut changes = Vec::new();
    if let Some(prs) = &project.roles.prs {
        let tasks = user.project_tasks(project.id).await?;
        let review = tasks
            .iter()
            .filter(|task| task.state == TaskState::Review)
            .filter_map(|task| Some((task, pr_number(task.pr_url.as_deref()?)?)))
            .take(MAX_CHECKED);
        for (task, number) in review {
            let pr = match pr_status(fleet, user, project, prs, number).await {
                Ok(Some(pr)) => pr,
                Ok(None) => continue,
                Err(error) => {
                    log::info!("projects: unable to read a pull request: {error:#}");
                    continue;
                }
            };
            let fingerprint = pr_fingerprint(&pr);
            if task.pr_seen.as_deref() != Some(fingerprint.as_str()) {
                user.set_pr_seen(task.id, &fingerprint).await?;
                changes.push(pr_line(task, &pr));
            }
            if pr.merged {
                finish_task(fleet, user, project, task).await?;
            }
        }
    }
    if !changes.is_empty() {
        let chat = user.ensure_project_agent(project.id).await?;
        fleet.post(
            chat,
            CHECK_LABEL,
            format!(
                "Pull requests changed:\n{}\n\nTell the user what needs them, if anything: a \
                 failing check or requested changes go back to the task's worker.",
                changes.join("\n")
            ),
        );
    }
    Ok(())
}

async fn rearm_check(
    fleet: &dyn Fleet,
    user: &User,
    project: ProjectId,
    now: DateTime<Utc>,
) -> Result<()> {
    let open = user
        .project_tasks(project)
        .await?
        .iter()
        .any(|task| task.state.wants_checks());
    let due = Due::Check(project);
    if open {
        fleet.projects().arm(due, after(now, CHECK_EVERY));
    } else {
        fleet.projects().disarm(&due);
    }
    Ok(())
}

async fn pr_status(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    prs: &IssueHost,
    number: u64,
) -> Result<Option<PrState>> {
    let home = fleet::home_shell(fleet, user, project)
        .await
        .map_err(anyhow::Error::msg)?;
    let command = HostCommand::PrStatus { number };
    let out = timeout(PR_LIMIT, run_host(home.as_ref(), prs, project, command))
        .await
        .context("the host did not answer in time")??;
    parse_pr(prs, &out)
}

fn pr_line(task: &TaskRow, pr: &PrState) -> String {
    let state = if pr.merged {
        "merged"
    } else if pr.open {
        "open"
    } else {
        "closed"
    };
    let checks: &'static str = pr.checks.into();
    let mut line = format!("- {}: {} is {state}, checks {checks}", task.slug, pr.url);
    if let Some(review) = &pr.review {
        let _ = write!(line, ", review {}", review.to_lowercase().replace('_', " "));
    }
    line
}

/// Closes `worker` if still idle at `now`: waiting, not asking, last turn [`IDLE_CLOSE`] ago.
pub(crate) async fn idle_close(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    worker: WorkerId,
    now: DateTime<Utc>,
) -> Result<()> {
    let Some(Working {
        worker,
        task,
        project,
        chat,
        machine,
    }) = working(fleet, user, worker).await?
    else {
        return Ok(());
    };
    // Working again, or asking: a later turn arms it again.
    if worker.state != WorkerState::Waiting || worker.asking {
        return Ok(());
    }
    let Some(last) = worker.last_turn_at else {
        return Ok(());
    };
    let due = after(last, IDLE_CLOSE);
    if due > now {
        fleet.projects().arm(Due::Idle(worker.id), due);
        return Ok(());
    }
    // A turn steered outside project_worker leaves the worker waiting while
    // its session works: the session says whether it is idle.
    match fleet.session_info(&worker).await {
        Ok(info)
            if matches!(
                info.state.status,
                Status::Starting | Status::Working | Status::Asking
            ) =>
        {
            fleet
                .projects()
                .arm(Due::Idle(worker.id), after(now, IDLE_CLOSE));
            return Ok(());
        }
        Ok(_) => {}
        Err(error) => log::info!("projects: closing an idle worker unseen: {error:#}"),
    }
    close(fleet, user, &worker).await?;
    block(fleet, user, &project, &task).await?;
    let stays = if task.state == TaskState::Running {
        "Its task is blocked until it starts again."
    } else {
        "Its task stays as it was."
    };
    fleet.post(
        chat,
        WORKER_LABEL,
        format!(
            "Closed the worker for {} on {machine}: it was idle for {} minutes with nothing \
             asked. {stays}",
            task.slug,
            IDLE_CLOSE.as_secs() / 60
        ),
    );
    fill_slots(fleet, user).await
}

/// Ends `worker` and closes its session, untracked first so its end is not taken as news.
async fn close(fleet: &dyn Fleet, user: &User, worker: &WorkerRow) -> Result<()> {
    fleet.projects().untrack(worker.id);
    if let Some(session) = &worker.session
        && let Err(error) = fleet
            .close_session(&worker.machine, worker.agent, session)
            .await
    {
        log::warn!("projects: unable to close a worker's session: {error:#}");
    }
    user.set_worker_state(worker.id, WorkerState::Ended).await?;
    Ok(())
}

/// The task is done: closes its issue, ends its worker, removes its worktree, frees the slot.
pub(crate) async fn finish_task(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    project: &ProjectRow,
    task: &TaskRow,
) -> Result<()> {
    user.set_task_state(task.id, TaskState::Done).await?;
    if let Some(issue) = &task.issue
        && task.issue_foreign
        && task.state != TaskState::Done
    {
        // Someone else's issue stays open; one comment may go, after asking.
        let chat = user.ensure_project_agent(project.id).await?;
        let mut text = format!(
            "{} is done. Its issue #{} was opened by someone else, so Beep leaves it open.",
            task.slug, issue.number
        );
        if !task.issue_commented {
            text.push_str(
                " If a note there helps, offer the user one comment with project_tasks comment.",
            );
        }
        fleet.post(chat, WORKER_LABEL, text);
    } else if let (Some(issue), Some(host)) = (&task.issue, &task.issue_host)
        && task.state != TaskState::Done
    {
        let comment = match &task.pr_url {
            Some(url) => format!("Done in {url}"),
            None => "Done.".to_owned(),
        };
        let command = HostCommand::CloseIssue {
            number: issue.number,
            comment: &comment,
            not_planned: false,
        };
        match fleet::home_shell(fleet, user, project).await {
            Ok(home) => {
                if let Err(error) = run_host(home.as_ref(), host, project, command).await {
                    log::warn!("projects: unable to close a done task's issue: {error:#}");
                }
            }
            Err(why) => log::info!("projects: a done task's issue stays open: {why}"),
        }
    }
    let worker = user
        .project_workers(project.id, task.created_at)
        .await?
        .into_iter()
        .rfind(|worker| worker.task == task.id);
    if let Some(worker) = worker {
        if worker.state.is_live() {
            close(fleet, user, &worker).await?;
        }
        remove_worktree(fleet, user, project, task, &worker).await?;
    }
    fill_slots(fleet, user).await
}

async fn remove_worktree(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    task: &TaskRow,
    worker: &WorkerRow,
) -> Result<()> {
    let checkout = user
        .checkouts(project.id)
        .await?
        .into_iter()
        .find(|checkout| {
            checkout.machine == worker.machine
                && task.slug.worktree(&checkout.path) == worker.folder
        });
    let Some(checkout) = checkout else {
        return Ok(());
    };
    match fleet.shell(&worker.machine) {
        Some(shell) => {
            let out = shell
                .run(
                    &worktree_remove(&checkout.path, &task.slug),
                    super::issues::HOST_LIMIT,
                )
                .await?;
            if out.code != Some(0) {
                log::warn!(
                    "projects: unable to remove a done task's worktree: {}",
                    out.out
                );
            }
        }
        None => {
            let machines = fleet.machines().await?;
            let chat = user.ensure_project_agent(project.id).await?;
            fleet.post(
                chat,
                WORKER_LABEL,
                format!(
                    "{} is done. Its worktree on {} stays until that machine is back online.",
                    task.slug,
                    fleet::name_of(&machines, &worker.machine)
                ),
            );
        }
    }
    Ok(())
}

/// [`fill_slots`] after a slot freed; a failure is only logged.
pub(crate) async fn refill(fleet: &dyn Fleet, user: &Arc<User>) {
    if let Err(error) = fill_slots(fleet, user).await {
        log::warn!("projects: queued tasks not started: {error:#}");
    }
}

/// Starts the user's queued tasks, oldest first, until the slots are full.
pub(crate) async fn fill_slots(fleet: &dyn Fleet, user: &Arc<User>) -> Result<()> {
    let full = StartRefused::UserFull.to_string();
    for task in user.queued_tasks().await?.into_iter().take(MAX_FILLS) {
        let Some(request) = task.start.clone() else {
            continue;
        };
        let Some(project) = user.project(task.project).await? else {
            continue;
        };
        match fleet.start_task(&project, &task, &request).await {
            Ok(Started::Running(worker)) => {
                let chat = user.ensure_project_agent(project.id).await?;
                fleet.post(
                    chat,
                    WORKER_LABEL,
                    format!(
                        "A slot freed, so {} started with {} in {}.",
                        task.slug, worker.agent, worker.folder
                    ),
                );
            }
            Ok(Started::Queued(why)) if why == full => break,
            Ok(Started::Queued(_)) => {}
            Err(error) => {
                log::info!("projects: a queued task did not start: {error}");
                start_failed(fleet, user, &project, &task, &error.to_string()).await?;
            }
        }
    }
    Ok(())
}

/// A start of `task` failed: drops its start request and tells the project chat once.
pub(crate) async fn start_failed(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    task: &TaskRow,
    why: &str,
) -> Result<()> {
    user.set_task_start(task.id, None).await?;
    let chat = user.ensure_project_agent(project.id).await?;
    fleet.post(
        chat,
        REPLAN_LABEL,
        format!(
            "{} could not start: {why} It will not be tried again on its own.",
            task.slug
        ),
    );
    Ok(())
}

/// At server start: fails workers left starting and arms pending checks and idle closes.
pub(crate) async fn startup(app: &Arc<App>) -> Result<()> {
    for worker in app.store.fail_starting_workers().await? {
        let Some(fleet) = fleet_of_worker(app, worker.id).await? else {
            continue;
        };
        let user = fleet.user.clone();
        let Some(Working {
            task,
            project,
            chat,
            machine,
            ..
        }) = working(&fleet, &user, worker.id).await?
        else {
            continue;
        };
        if task.state.is_open() && task.state != TaskState::Review {
            user.set_task_state(task.id, TaskState::Blocked).await?;
            relabel(
                &fleet,
                &user,
                &project,
                &task,
                Some(LABEL_BLOCKED),
                &[LABEL_RUNNING],
            )
            .await;
        }
        fleet.post(
            chat,
            REPLAN_LABEL,
            format!(
                "The worker for {} on {machine} did not start: the server restarted while it was \
                 starting.",
                task.slug
            ),
        );
    }
    let now = Utc::now();
    for project in app.store.projects_wanting_checks().await? {
        app.projects
            .arm(Due::Check(project), after(now, CHECK_EVERY));
    }
    for worker in app.store.waiting_workers().await? {
        let last = worker.last_turn_at.unwrap_or(now);
        app.projects
            .arm(Due::Idle(worker.id), after(last, IDLE_CLOSE));
    }
    Ok(())
}

fn quote(text: &str) -> String {
    match text.char_indices().nth(MAX_QUOTED) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests;
