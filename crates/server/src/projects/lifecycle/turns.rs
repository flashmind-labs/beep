//! A worker's turns: news from its agent, finished turns, questions, blocks and its end.

use super::{
    super::{
        fleet::{self, Fleet},
        gate::ProjectGate,
        model::{CHECK_EVERY, IDLE_CLOSE, TaskState, WorkerId, WorkerState},
        policy::{Verdict, WorkerScope},
        schedule::Due,
        turn::{REPLAN_LABEL, WORKER_LABEL},
    },
    after, fill_slots, fleet_of_worker, quote,
};
use crate::{
    account::user::User,
    app::App,
    nodes::News,
    protocol::node::agent::{OptionKind, Question, Request as AgentRequest, SessionInfo, Status},
    store::{ProjectRow, TaskRow, WorkerRow},
};
use anyhow::{Context, Result};
use beep_projects::{
    commands::{
        HostCommand, LABEL_BLOCKED, LABEL_REVIEW, LABEL_RUNNING, PrState, parse_pr, pr_fingerprint,
    },
    issues::run_host,
    model::IssueHost,
};
use chrono::{DateTime, Utc};
use std::{fmt::Write, sync::Arc};
use uuid::Uuid;

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

pub(super) async fn catch_up(
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
pub(super) struct Working {
    pub(super) worker: WorkerRow,
    pub(super) task: TaskRow,
    pub(super) project: ProjectRow,
    pub(super) chat: Uuid,
    /// The worker's machine, as commands name it.
    pub(super) machine: String,
}

pub(super) async fn working(
    fleet: &dyn Fleet,
    user: &User,
    worker: WorkerId,
) -> Result<Option<Working>> {
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
pub(super) async fn find_pr(
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
    let out = run_host(home.as_ref(), prs, &project.name, command).await?;
    parse_pr(prs, &out)
}

/// Moves the task's issue labels; a failure only costs the label.
pub(super) async fn relabel(
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
    if let Err(error) = run_host(home.as_ref(), host, &project.name, command).await {
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
pub(super) fn option_of(question: &Question, kinds: &[OptionKind]) -> Option<String> {
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
pub(super) async fn handle_question(
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

pub(super) fn task_state_name(state: TaskState) -> &'static str {
    state.into()
}
