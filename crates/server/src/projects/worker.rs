//! Starting a task's worker in its own worktree, and stopping it.

use super::{
    fleet::{self, Fleet, Machine, RecordedFleet},
    lifecycle,
    model::{StartRequest, TaskState, WorkerAgent, WorkerState},
    place::{MachineFit, PlaceError, PlaceRequest, place},
    turn::read_personas,
};
use crate::{
    account::user::User,
    app::App,
    protocol::node::Capability,
    store::{CheckoutRow, ProjectRow, TaskRow, WorkerRow},
    tools::machines::command_name,
};
use anyhow::{Context, Result};
use beep_projects::model::TaskSlug;
use std::sync::Arc;

/// Most input files one start or prompt copies.
pub(crate) const MAX_INPUTS: usize = 16;

/// What a start did.
#[derive(Debug)]
pub(crate) enum Started {
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
    /// The machine's name, as commands take it.
    pub(crate) machine_name: String,
    pub(crate) agent: WorkerAgent,
    pub(crate) persona: Option<ChosenPersona>,
    /// The branch the worktree starts from.
    pub(crate) base: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ChosenPersona {
    pub(crate) name: String,
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
    let fleet = RecordedFleet::new(app, user);
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
    // No machine offers an agent to the server, so none is picked for the start.
    let agent = request
        .agent
        .or(engine)
        .ok_or_else(|| StartError::Agent(fleet::UNREACHABLE.to_owned()))?;
    let fits = fits(&fleet, &machines, &checkouts).await?;
    let at = place(
        &PlaceRequest {
            named: request.machine.as_deref(),
            pin: project.pin.as_ref(),
            needs: &needs,
        },
        &fits,
    )?;
    let fit = &fits[at];
    Ok(Prepared::Ready(Placement {
        machine_name: command_name(&fit.name),
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
    fleet: &RecordedFleet,
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
    Ok(FoundPersona {
        chosen: ChosenPersona {
            name: found.persona.name.clone(),
        },
        engine: found.engine,
        needs: found.needs.clone(),
    })
}

/// What placement knows about each machine for a worker of `agent`. None is online to the
/// server, so none offers an agent.
async fn fits(
    fleet: &RecordedFleet,
    machines: &[Machine],
    checkouts: &[CheckoutRow],
) -> Result<Vec<MachineFit>> {
    let live = fleet.user.live_workers().await?;
    let mut fits = Vec::with_capacity(machines.len());
    for machine in machines {
        // The home checkout first, when the machine has it.
        let checkout = checkouts
            .iter()
            .filter(|checkout| checkout.machine == machine.id)
            .max_by_key(|checkout| checkout.home)
            .map(|checkout| checkout.path.clone());
        let running = live
            .iter()
            .filter(|worker| worker.machine == machine.id)
            .count();
        fits.push(MachineFit {
            machine: machine.id,
            name: machine.name.clone(),
            online: machine.online,
            offers_agent: false,
            checkout,
            shared: false,
            capabilities: machine.capabilities.clone(),
            running: u32::try_from(running)?,
        });
    }
    Ok(fits)
}

/// Carries out a prepared start: a task that waits is queued; the server starts no agent, since
/// it reaches no machine.
pub(crate) async fn launch(
    _app: &Arc<App>,
    user: &Arc<User>,
    _project: &ProjectRow,
    task: &TaskRow,
    request: &StartRequest,
    prepared: Prepared,
) -> Result<Started, StartError> {
    match prepared {
        Prepared::Waits(after) => {
            user.set_task_start(task.id, Some(request)).await?;
            user.set_task_state(task.id, TaskState::Blocked).await?;
            Ok(Started::Queued(format!("waits for {after}")))
        }
        Prepared::Ready(_) => Err(StartError::Node(fleet::UNREACHABLE.to_owned())),
    }
}

/// Stops `worker`: closes and untracks its session, ends it, blocks a running task. No-op if ended.
pub(crate) async fn stop_worker(
    app: &Arc<App>,
    user: &Arc<User>,
    worker: &WorkerRow,
) -> Result<()> {
    let fleet = RecordedFleet::new(app, user);
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
