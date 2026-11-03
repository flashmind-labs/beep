//! Starting a task's worker in its own worktree, and stopping it.

use super::{
    commands::{
        HostCommand, LABEL_BLOCKED, LABEL_RUNNING, Shell, exclude_project_dirs, head_commit,
        worktree_add,
    },
    fleet::{self, Fleet, Machine, NodeFleet},
    issues::{HOST_LIMIT, run_host},
    lifecycle,
    model::{
        InputFile, IssueHost, IssueRef, MachinePath, RepoName, StartRequest, TaskSlug, TaskState,
        WorkerAgent, WorkerState,
    },
    place::{MachineFit, PlaceError, PlaceRequest, place},
    service::{Job, JobKind},
    turn::read_personas,
};
use crate::{
    app::App,
    nodes::{Follower, Online, SHELL, StartOn},
    protocol::node::{
        Capability,
        files::{FileKind, Reply, Request},
    },
    store::{CheckoutRow, NewWorker, ProjectRow, StartRefused, TaskRow, WorkerRow},
    tools::machines::{
        Machines, Need, Picked, command_name,
        copy::{self, FileNode, MAX_COPY},
        transfer,
    },
    user::User,
};
use anyhow::{Context, Result, anyhow};
use iroh::EndpointId;
use std::{fmt::Write, str::FromStr, sync::Arc, time::Duration};
use strum::VariantNames;
use tokio_util::sync::CancellationToken;

/// Most input files one start or prompt copies.
pub(crate) const MAX_INPUTS: usize = 16;
/// How long making the worktree may take.
const NODE_LIMIT: Duration = Duration::from_secs(120);
/// How much of a failed command's output the failure shows.
const TAIL: usize = 400;

/// What a start did.
#[derive(Debug)]
pub(crate) enum Started {
    Running(Box<WorkerRow>),
    /// Not started; the text says why and the request is kept.
    Queued(String),
}

/// Why a start failed; the text is for the model.
#[derive(Debug, thiserror::Error)]
pub(crate) enum StartError {
    #[error(transparent)]
    Place(#[from] PlaceError),
    #[error("{0}")]
    Persona(String),
    #[error("{0}")]
    Agent(String),
    #[error("{0}")]
    Node(String),
    #[error("{0:#}")]
    Store(#[from] anyhow::Error),
}

/// A start worked out, before anything changed.
#[derive(Debug)]
pub(crate) enum Prepared {
    /// The task waits for the task with this slug to reach review.
    Waits(TaskSlug),
    Ready(Placement),
}

/// Where and how a worker would run.
#[derive(Debug, Clone)]
pub(crate) struct Placement {
    pub(crate) machine: EndpointId,
    /// The machine's name, as commands take it.
    pub(crate) machine_name: String,
    pub(crate) checkout: MachinePath,
    pub(crate) agent: WorkerAgent,
    pub(crate) persona: Option<ChosenPersona>,
    /// The branch the worktree starts from.
    pub(crate) base: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ChosenPersona {
    pub(crate) name: String,
    pub(crate) prompt: String,
    /// The home checkout's commit it was read at.
    pub(crate) commit: Option<String>,
}

/// The node's connection, only while it is signed in as `user`.
fn online(app: &App, user: &User, machine: &EndpointId) -> Option<Online> {
    app.nodes
        .online(&machine.to_string())
        .filter(|online| online.user == user.id())
}

/// The end of `text`, cut on a character boundary.
fn tail(text: &str) -> String {
    let text = text.trim();
    let mut start = text.len().saturating_sub(TAIL);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_owned()
}

/// Starts `task` per `request`: waits for its dependency, else places and starts the agent.
pub(crate) async fn start_task(
    app: &Arc<App>,
    user: &Arc<User>,
    project: &ProjectRow,
    task: &TaskRow,
    request: &StartRequest,
) -> Result<Started, StartError> {
    let prepared = prepare(app, user, project, task, request).await?;
    launch(app, user, project, task, request, prepared).await
}

/// Works out where and how `task` would start, changing nothing.
pub(crate) async fn prepare(
    app: &Arc<App>,
    user: &Arc<User>,
    project: &ProjectRow,
    task: &TaskRow,
    request: &StartRequest,
) -> Result<Prepared, StartError> {
    let fleet = NodeFleet::new(app, user);
    let mut base = request
        .base
        .clone()
        .unwrap_or_else(|| project.default_branch.clone());
    if let Some(after) = task.depends_on {
        let after = user
            .project_task(after)
            .await?
            .context("the task it waits for is gone")?;
        match after.state {
            TaskState::Done => {}
            TaskState::Review => base = after.branch.unwrap_or_else(|| after.slug.branch()),
            _ => return Ok(Prepared::Waits(after.slug)),
        }
    }
    let checkouts = user.checkouts(project.id).await?;
    let persona = match &request.persona {
        Some(name) => Some(find_persona(&fleet, &checkouts, name).await?),
        None => None,
    };
    let mut needs = task.needs.clone();
    for need in persona.iter().flat_map(|found| &found.needs) {
        if !needs.contains(need) {
            needs.push(*need);
        }
    }
    let machines = fleet.machines().await?;
    let engine = persona.as_ref().and_then(|found| found.engine);
    let agent = match request.agent.or(engine) {
        Some(agent) => agent,
        None => only_agent(app, user, &machines, &checkouts)?,
    };
    let fits = fits(&fleet, &machines, &checkouts, agent).await?;
    let at = place(
        &PlaceRequest {
            named: request.machine.as_deref(),
            pin: project.pin.as_ref(),
            needs: &needs,
        },
        &fits,
    )?;
    let fit = &fits[at];
    let checkout = fit
        .checkout
        .clone()
        .ok_or_else(|| anyhow!("the machine placed has no checkout"))?;
    Ok(Prepared::Ready(Placement {
        machine: fit.machine,
        machine_name: command_name(&fit.name),
        checkout,
        agent,
        persona: persona.map(|found| found.chosen),
        base,
    }))
}

/// A persona as a start uses it.
struct FoundPersona {
    chosen: ChosenPersona,
    engine: Option<WorkerAgent>,
    needs: Vec<Capability>,
}

/// The persona `name` from the home checkout's `.agents/`.
async fn find_persona(
    fleet: &NodeFleet,
    checkouts: &[CheckoutRow],
    name: &str,
) -> Result<FoundPersona, StartError> {
    let Some(home) = checkouts.iter().find(|checkout| checkout.home) else {
        return Err(StartError::Persona(
            "The project has no home checkout to read personas from.".to_owned(),
        ));
    };
    let personas: Vec<_> = read_personas(fleet, home)
        .await
        .into_iter()
        .filter_map(|(_, parsed)| parsed.ok())
        .collect();
    let Some(found) = personas.iter().find(|found| found.persona.name == name) else {
        let names: Vec<&str> = personas
            .iter()
            .map(|found| found.persona.name.as_str())
            .collect();
        return Err(StartError::Persona(if names.is_empty() {
            format!("There is no persona {name}: the home checkout has none in .agents/.")
        } else {
            format!(
                "There is no persona {name}. Personas: {}.",
                names.join(", ")
            )
        }));
    };
    // The commit only notes which version ran; a start goes on without it.
    let commit = match fleet.shell(&home.machine) {
        Some(shell) => match shell.run(&head_commit(&home.path), HOST_LIMIT).await {
            Ok(out) if out.code == Some(0) => Some(out.out.trim().to_owned()),
            Ok(out) => {
                log::warn!(
                    "projects: unable to read the persona commit: {}",
                    tail(&out.out)
                );
                None
            }
            Err(error) => {
                log::warn!("projects: unable to read the persona commit: {error:#}");
                None
            }
        },
        None => None,
    };
    Ok(FoundPersona {
        chosen: ChosenPersona {
            name: found.persona.name.clone(),
            prompt: found.persona.system_prompt.clone(),
            commit,
        },
        engine: found.engine,
        needs: found.needs.clone(),
    })
}

/// The one agent the machines with a checkout offer, when it is one.
fn only_agent(
    app: &App,
    user: &User,
    machines: &[Machine],
    checkouts: &[CheckoutRow],
) -> Result<WorkerAgent, StartError> {
    let onlines: Vec<Online> = machines
        .iter()
        .filter(|machine| checkouts.iter().any(|c| c.machine == machine.id))
        .filter_map(|machine| online(app, user, &machine.id))
        .collect();
    let offered: Vec<WorkerAgent> = WorkerAgent::VARIANTS
        .iter()
        .filter_map(|name| WorkerAgent::from_str(name).ok())
        .filter(|agent| {
            onlines
                .iter()
                .any(|online| online.offers(&agent.protocol()))
        })
        .collect();
    match offered.as_slice() {
        [agent] => Ok(*agent),
        [] => Err(StartError::Agent(
            "No machine with a checkout of the project offers a coding agent.".to_owned(),
        )),
        several => {
            let names: Vec<String> = several.iter().map(ToString::to_string).collect();
            Err(StartError::Agent(format!(
                "Say which agent runs it: {} are offered.",
                names.join(", ")
            )))
        }
    }
}

/// What placement knows about each machine for a worker of `agent`.
async fn fits(
    fleet: &NodeFleet,
    machines: &[Machine],
    checkouts: &[CheckoutRow],
    agent: WorkerAgent,
) -> Result<Vec<MachineFit>> {
    let live = fleet.user.live_workers().await?;
    let mut fits = Vec::with_capacity(machines.len());
    for machine in machines {
        let online = online(&fleet.app, &fleet.user, &machine.id);
        // The home checkout first, when the machine has it.
        let checkout = checkouts
            .iter()
            .filter(|checkout| checkout.machine == machine.id)
            .max_by_key(|checkout| checkout.home)
            .map(|checkout| checkout.path.clone());
        let shared = match (&online, &checkout) {
            (Some(_), Some(path)) => fleet::shared(fleet, &machine.id, path)
                .await
                .unwrap_or_else(|error| {
                    log::warn!("projects: unable to read a machine's shared folders: {error:#}");
                    false
                }),
            _ => false,
        };
        let running = live
            .iter()
            .filter(|worker| worker.machine == machine.id)
            .count();
        fits.push(MachineFit {
            machine: machine.id,
            name: machine.name.clone(),
            online: online.is_some(),
            offers_agent: online
                .as_ref()
                .is_some_and(|online| online.offers(&agent.protocol())),
            checkout,
            shared,
            capabilities: machine.capabilities.clone(),
            running: u32::try_from(running)?,
        });
    }
    Ok(fits)
}

/// Carries out a prepared start.
pub(crate) async fn launch(
    app: &Arc<App>,
    user: &Arc<User>,
    project: &ProjectRow,
    task: &TaskRow,
    request: &StartRequest,
    prepared: Prepared,
) -> Result<Started, StartError> {
    let placement = match prepared {
        Prepared::Waits(after) => {
            user.set_task_start(task.id, Some(request)).await?;
            user.set_task_state(task.id, TaskState::Blocked).await?;
            return Ok(Started::Queued(format!("waits for {after}")));
        }
        Prepared::Ready(placement) => placement,
    };
    let worktree = task.slug.worktree(&placement.checkout);
    let new = NewWorker {
        task: task.id,
        machine: placement.machine,
        agent: placement.agent,
        folder: worktree.clone(),
        persona: placement.persona.as_ref().map(|found| found.name.clone()),
        persona_commit: placement
            .persona
            .as_ref()
            .and_then(|found| found.commit.clone()),
    };
    let worker = match user.start_worker(&new).await? {
        Ok(worker) => worker,
        // The task runs already: nothing to keep.
        Err(refused @ StartRefused::TaskHasWorker) => {
            return Ok(Started::Queued(refused.to_string()));
        }
        Err(refused) => {
            user.set_task_start(task.id, Some(request)).await?;
            user.set_task_state(task.id, TaskState::Planned).await?;
            return Ok(Started::Queued(refused.to_string()));
        }
    };
    let on_node = OnNode {
        app,
        user,
        project,
        task,
        placement: &placement,
        worktree: &worktree,
    };
    let (online, session) = match on_node.start(&request.inputs).await {
        Ok(started) => started,
        Err(why) => {
            user.set_worker_state(worker.id, WorkerState::Failed)
                .await?;
            user.set_task_state(task.id, TaskState::Blocked).await?;
            return Err(StartError::Node(why));
        }
    };
    user.set_worker_session(worker.id, &session).await?;
    user.set_worker_state(worker.id, WorkerState::Running)
        .await?;
    let chat = user.ensure_project_agent(project.id).await?;
    // Tracked before it reports to anyone, so its news reaches the project.
    app.projects.track(placement.machine, &session, worker.id);
    online.report_to(
        &session,
        Follower {
            user: user.clone(),
            chat,
            node: placement.machine_name.clone(),
            agent: placement.agent.to_string(),
        },
    );
    user.set_task_state(task.id, TaskState::Running).await?;
    user.set_task_branch(task.id, &task.slug.branch()).await?;
    relabel_running(app, user, project, task).await;
    user.set_task_start(task.id, None).await?;
    // What the session did before it was tracked, such as a first question.
    match online.agent_session(placement.agent.into(), &session).await {
        Ok(link) => {
            let info = link.now().info;
            let id = worker.id;
            let job = Job::new(JobKind::CatchUp, move |app| async move {
                lifecycle::caught_up(&app, id, info).await
            });
            app.projects.spawn_for(app, id, job);
        }
        Err(error) => log::warn!("projects: unable to read a new worker's session: {error:#}"),
    }
    let worker = user.project_worker(worker.id).await?.unwrap_or(worker);
    Ok(Started::Running(Box::new(worker)))
}

/// Marks the task's issue running; a failure only costs the label.
async fn relabel_running(app: &Arc<App>, user: &Arc<User>, project: &ProjectRow, task: &TaskRow) {
    let (Some(issue), Some(host)) = (&task.issue, &task.issue_host) else {
        return;
    };
    // Someone else's issue keeps its labels.
    if task.issue_foreign {
        return;
    }
    let fleet = NodeFleet::new(app, user);
    let home = match fleet::home_shell(&fleet, user, project).await {
        Ok(home) => home,
        Err(why) => {
            log::warn!("projects: the running label waits: {why}");
            return;
        }
    };
    let command = HostCommand::Relabel {
        number: issue.number,
        add: Some(LABEL_RUNNING),
        remove: &[LABEL_BLOCKED],
    };
    if let Err(error) = run_host(home.as_ref(), host, project, command).await {
        log::warn!("projects: unable to label the issue running: {error:#}");
    }
}

/// The node side of a start.
struct OnNode<'a> {
    app: &'a Arc<App>,
    user: &'a Arc<User>,
    project: &'a ProjectRow,
    task: &'a TaskRow,
    placement: &'a Placement,
    worktree: &'a MachinePath,
}

impl OnNode<'_> {
    /// Makes the worktree, copies inputs and starts the agent; the error is for the model.
    async fn start(&self, inputs: &[InputFile]) -> Result<(Online, String), String> {
        let placement = self.placement;
        let online = online(self.app, self.user, &placement.machine)
            .ok_or_else(|| format!("{} went offline.", placement.machine_name))?;
        if !online.offers(SHELL) {
            return Err(format!("{} offers no shell.", placement.machine_name));
        }
        run_checked(&online, &exclude_project_dirs(&placement.checkout)).await?;
        run_checked(
            &online,
            &worktree_add(&placement.checkout, &self.task.slug, &placement.base),
        )
        .await?;
        let incoming = self.task.slug.incoming(self.worktree);
        copy_inputs(
            self.app,
            self.user,
            &placement.machine_name,
            &incoming,
            inputs,
        )
        .await?;
        let task_prompt = task_prompt(&PromptFacts {
            repo: &self.project.name,
            issue: self.task.issue.as_ref(),
            issue_foreign: self.task.issue_foreign,
            title: &self.task.title,
            body: &self.task.body,
            slug: &self.task.slug,
            worktree: self.worktree,
            base: &placement.base,
            default_branch: &self.project.default_branch,
            has_inputs: !inputs.is_empty(),
            issues: self.project.roles.issues.as_ref(),
            prs: self.project.roles.prs.as_ref(),
        });
        let prompt = worker_prompt(
            placement
                .persona
                .as_ref()
                .map(|found| found.prompt.as_str()),
            &task_prompt,
        );
        let link = online
            .start_agent(
                placement.agent.into(),
                Some(self.worktree.to_string()),
                prompt,
                StartOn::default(),
            )
            .await
            .map_err(|error| format!("Could not start {}: {error:#}", placement.agent))?;
        let session = link.now().info.id;
        Ok((online, session))
    }
}

/// Runs `command` on the node, failing with its output's end.
async fn run_checked(online: &Online, command: &str) -> Result<(), String> {
    let out = online
        .run(command, NODE_LIMIT)
        .await
        .map_err(|error| format!("{error:#}"))?;
    match out.code {
        Some(0) => Ok(()),
        Some(code) => Err(format!("A command exited with {code}: {}", tail(&out.out))),
        None => Err(format!(
            "A command did not finish in {}s: {}",
            NODE_LIMIT.as_secs(),
            tail(&out.out)
        )),
    }
}

/// Copies each input into `incoming` on `to`; one without a machine comes from the user's
/// environment (or `to` when absent). Says where each landed.
pub(crate) async fn copy_inputs(
    app: &Arc<App>,
    user: &Arc<User>,
    to: &str,
    incoming: &MachinePath,
    inputs: &[InputFile],
) -> Result<Vec<String>, String> {
    if inputs.len() > MAX_INPUTS {
        return Err(format!("At most {MAX_INPUTS} input files go with a task."));
    }
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    let machines = Machines {
        app: app.clone(),
        user: user.clone(),
    };
    let target = machines.pick(Some(to), Need::Files).await?;
    target
        .online
        .make_dirs(incoming.as_str())
        .await
        .map_err(|error| format!("Could not make {incoming}: {error:#}"))?;
    let mut landed = Vec::with_capacity(inputs.len());
    for input in inputs {
        let source = match &input.machine {
            Some(name) => machines.pick(Some(name), Need::Files).await?,
            None => match machines.pick(None, Need::Files).await {
                Ok(environment) => environment,
                Err(_) => machines.pick(Some(to), Need::Files).await?,
            },
        };
        landed.push(
            copy_input(&source, &input.path, &target, incoming)
                .await
                .map_err(|error| format!("Could not copy {}: {error:#}", input.path))?,
        );
    }
    Ok(landed)
}

async fn copy_input(
    source: &Picked,
    path: &str,
    target: &Picked,
    incoming: &MachinePath,
) -> Result<String> {
    let src = copy::picked_path(source, path)?;
    let entry = match source
        .online
        .files(&Request::Stat { path: src.clone() })
        .await?
    {
        Reply::Stat { entry, .. } => entry,
        Reply::Error { message } => anyhow::bail!("{message}"),
        other => anyhow::bail!("the node answered {other:?}"),
    };
    anyhow::ensure!(entry.kind == FileKind::File, "{src} is not a file");
    anyhow::ensure!(
        entry.size <= MAX_COPY,
        "{src} is larger than {} GiB",
        MAX_COPY >> 30
    );
    if source.online.same_machine(&target.online) {
        let name = copy::file_name(&src).context("the path names no file")?;
        let dst = incoming.join(name);
        Ok(
            copy::copy(&source.online, &src, &target.online, dst.as_str(), true)
                .await?
                .path,
        )
    } else {
        let sent = transfer::send(
            source,
            &src,
            target,
            incoming.as_str(),
            &mut |_| {},
            &CancellationToken::new(),
        )
        .await?;
        Ok(sent.path)
    }
}

/// Stops `worker`: closes and untracks its session, ends it, blocks a running task. No-op if ended.
pub(crate) async fn stop_worker(
    app: &Arc<App>,
    user: &Arc<User>,
    worker: &WorkerRow,
) -> Result<()> {
    let fleet = NodeFleet::new(app, user);
    stop_worker_on(&fleet, user, worker).await
}

/// [`stop_worker`] through `fleet`, then refills the freed slot.
pub(crate) async fn stop_worker_on(
    fleet: &dyn Fleet,
    user: &Arc<User>,
    worker: &WorkerRow,
) -> Result<()> {
    end_worker(fleet, user, worker).await?;
    super::lifecycle::refill(fleet, user).await;
    Ok(())
}

/// [`stop_worker_on`] without starting queued tasks.
pub(crate) async fn end_worker(fleet: &dyn Fleet, user: &User, worker: &WorkerRow) -> Result<()> {
    let Some(current) = user.project_worker(worker.id).await? else {
        return Ok(());
    };
    // Forgotten first, so the end its session reports is not news.
    fleet.projects().untrack(current.id);
    if current.state.is_live() {
        if let Some(session) = &current.session
            && let Err(error) = fleet
                .close_session(&current.machine, current.agent, session)
                .await
        {
            // An offline machine ends the session when it is back.
            log::warn!("projects: unable to close a worker's session: {error:#}");
        }
        user.set_worker_state(current.id, WorkerState::Ended)
            .await?;
        if let Some(task) = user.project_task(current.task).await?
            && let Some(project) = user.project(task.project).await?
        {
            lifecycle::block(fleet, user, &project, &task).await?;
        }
    }
    Ok(())
}

/// What the task prompt is filled in from.
pub(crate) struct PromptFacts<'a> {
    pub(crate) repo: &'a RepoName,
    pub(crate) issue: Option<&'a IssueRef>,
    /// Someone else opened the issue: the PR refers to it without closing it.
    pub(crate) issue_foreign: bool,
    pub(crate) title: &'a str,
    pub(crate) body: &'a str,
    pub(crate) slug: &'a TaskSlug,
    pub(crate) worktree: &'a MachinePath,
    pub(crate) base: &'a str,
    pub(crate) default_branch: &'a str,
    pub(crate) has_inputs: bool,
    pub(crate) issues: Option<&'a IssueHost>,
    pub(crate) prs: Option<&'a IssueHost>,
}

/// What the worker is told about its task.
pub(crate) fn task_prompt(facts: &PromptFacts<'_>) -> String {
    let branch = facts.slug.branch();
    let default_branch = facts.default_branch;
    let tracked = match facts.issue {
        Some(issue) if issue.url.is_empty() => format!("issue #{}", issue.number),
        Some(issue) => format!("issue #{} ({})", issue.number, issue.url),
        None => "a task in Beep".to_owned(),
    };
    let mut prompt = format!(
        "You are working on one task of the project {}, tracked as {tracked}.\n\n",
        facts.repo
    );
    let _ = writeln!(prompt, "Task: {}", facts.title.trim());
    let _ = writeln!(prompt, "{}\n", facts.body.trim());
    let _ = writeln!(
        prompt,
        "You are in a git worktree at {} on branch {branch}, made from {}. Work only in this \
         worktree.",
        facts.worktree, facts.base
    );
    if facts.has_inputs {
        let _ = writeln!(
            prompt,
            "The input files are in .beep/incoming/{}/ in the worktree.",
            facts.slug
        );
    }
    prompt.push('\n');
    prompt.push_str("When the work is done and its checks pass:\n");
    let _ = writeln!(prompt, "1. Commit it on {branch}.");
    let _ = writeln!(prompt, "2. Push the branch: git push -u origin {branch}");
    if let Some(prs) = facts.prs {
        let issue_link = match facts.issue {
            Some(issue) if facts.issue_foreign && facts.issues == Some(prs) => {
                format!(" with \"Refs #{}\" in its description", issue.number)
            }
            Some(issue) if facts.issues == Some(prs) => {
                format!(" with \"Closes #{}\" in its description", issue.number)
            }
            Some(issue) => format!(" linking {}", issue.url),
            None => String::new(),
        };
        let _ = writeln!(
            prompt,
            "3. Open a pull request from {branch} into {default_branch}{issue_link}."
        );
    } else {
        prompt.push_str(
            "3. Do not open a pull request; say in your last message that the branch is pushed.\n",
        );
    }
    prompt.push('\n');
    let _ = writeln!(
        prompt,
        "Never push to {default_branch}, never force push, and never merge. Write the pull \
         request for a public audience: no machine names, user names, home folders, IP addresses \
         or ports. Files you make that git ignores are copied back on request."
    );
    prompt
}

/// The persona's prompt, a blank line, then the task's.
pub(crate) fn worker_prompt(persona: Option<&str>, task: &str) -> String {
    match persona {
        Some(persona) => format!("{}\n\n{task}", persona.trim_end()),
        None => task.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        repo: RepoName,
        slug: TaskSlug,
        worktree: MachinePath,
        issue: IssueRef,
    }

    fn fixture() -> Fixture {
        let slug = TaskSlug::from_title("Fix it");
        let checkout = MachinePath::parse("/srv/widget").expect("a path");
        Fixture {
            repo: RepoName::parse("acme/widget").expect("a repo"),
            worktree: slug.worktree(&checkout),
            slug,
            issue: IssueRef {
                number: 12,
                url: "https://github.com/acme/widget/issues/12".into(),
            },
        }
    }

    fn facts(fixture: &Fixture) -> PromptFacts<'_> {
        PromptFacts {
            repo: &fixture.repo,
            issue: Some(&fixture.issue),
            issue_foreign: false,
            title: "Fix it",
            body: "Make the build pass.",
            slug: &fixture.slug,
            worktree: &fixture.worktree,
            base: "main",
            default_branch: "main",
            has_inputs: true,
            issues: Some(&IssueHost::Github),
            prs: Some(&IssueHost::Github),
        }
    }

    #[test]
    fn a_foreign_issue_is_referred_to_never_closed() {
        let fixture = fixture();
        assert!(task_prompt(&facts(&fixture)).contains("\"Closes #12\""));
        let foreign = task_prompt(&PromptFacts {
            issue_foreign: true,
            ..facts(&fixture)
        });
        assert!(
            foreign.contains("with \"Refs #12\" in its description"),
            "{foreign}"
        );
        assert!(!foreign.contains("Closes"), "{foreign}");
    }
}
