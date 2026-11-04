//! A project's page: its machines and sessions, and the project agent's
//! tasks, workers and chat.

use super::{import::outdated, known};
use crate::{
    agents::sessions::SessionRow,
    beep::{BarEvent, Pick, Sent, Service, Turn, epoch_ms},
    center::{self, Chat},
    error::{DesktopError, Result},
    fleet::projects::{FleetState, desk, desk_or_server},
};
use beep_fleet::project_id;
use beep_proto::{
    ChatApproval, CopyProject, GetProjectAgent, ListPendingApprovals, ListProjectMachines,
    ListProjectSessions, ListProjectSyncthing, ListProjectTasks, ListProjectWorkers,
    PendingApproval, ProjectAgent, ProjectCopy, ProjectMachine, ProjectTask, ProjectTaskState,
    ProjectWorker, ProjectWorkerState, StopProjectWorker, SyncthingPlace, UpdateProjectTask,
};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use tauri::{WebviewWindow, ipc::Channel};
use tokio::join;
use uuid::Uuid;

/// The sessions coding agents ran in `project`'s folders, newest first;
/// none from a server too old to list them.
#[tauri::command]
pub async fn project_sessions(beep: Service<'_>, project: String) -> Result<Vec<SessionRow>> {
    let sessions = known(beep.call(ListProjectSessions { project }).await)?;
    Ok(sessions
        .into_iter()
        .flatten()
        .map(SessionRow::from)
        .collect())
}

/// One of the user's machines as a project's page lists it.
#[derive(Debug, Serialize)]
pub struct ProjectMachineRow {
    id: String,
    name: String,
    online: bool,
    /// The project's folder there; `None` without a checkout.
    folder: Option<String>,
    /// The server is looking through it for a checkout now.
    checking: bool,
    /// In milliseconds since the Unix epoch.
    checked_at: Option<u64>,
}

impl From<ProjectMachine> for ProjectMachineRow {
    fn from(m: ProjectMachine) -> Self {
        Self {
            id: m.machine_id,
            name: m.machine_name,
            online: m.online,
            folder: m.folder,
            checking: m.checking,
            checked_at: m.checked_at.map(epoch_ms),
        }
    }
}

/// Every machine of the user's for `project`'s page: its folder there, or
/// whether the server is looking for one. `None` from a server too old to
/// say, and the page lists the imported folders as before.
#[tauri::command]
pub async fn project_machines(
    fleet: FleetState<'_>,
    beep: Service<'_>,
    project: String,
) -> Result<Option<Vec<ProjectMachineRow>>> {
    // A project only the server has: until it is moved into the mesh, the server answers.
    let desk = match desk_or_server(&fleet, &beep).await? {
        Some(desk) if desk.has_project(&project) || beep.server().is_none() => desk,
        _ => {
            let machines = known(beep.call(ListProjectMachines { project }).await)?;
            return Ok(machines
                .map(|machines| machines.into_iter().map(ProjectMachineRow::from).collect()));
        }
    };
    let machines = desk.project_machines(&project).await?;
    Ok(Some(
        machines.into_iter().map(ProjectMachineRow::from).collect(),
    ))
}

/// Copies `project` onto `machine_id` through the server, which brings it
/// over sync. `None` from a server too old to, or with no server, and the
/// page clones instead.
#[tauri::command]
pub async fn project_copy(
    beep: Service<'_>,
    project: String,
    machine_id: String,
) -> Result<Option<ProjectCopy>> {
    match known(
        beep.call(CopyProject {
            project,
            machine_id,
        })
        .await,
    ) {
        Err(DesktopError::NoServer) => Ok(None),
        copied => copied,
    }
}

/// The machines on which Syncthing keeps `project`'s folder in step. Empty
/// from a server too old to say.
#[tauri::command]
pub async fn project_syncthing(
    fleet: FleetState<'_>,
    beep: Service<'_>,
    project: String,
) -> Result<Vec<SyncthingPlace>> {
    match desk_or_server(&fleet, &beep).await? {
        Some(desk) if desk.has_project(&project) || beep.server().is_none() => {
            Ok(desk.syncthing(&project).await)
        }
        _ => Ok(known(beep.call(ListProjectSyncthing { project }).await)?.unwrap_or_default()),
    }
}

/// The project agent of `name` (`org/repo`), made with its chat on the
/// first ask.
#[tauri::command]
pub async fn center_open_project(
    fleet: FleetState<'_>,
    beep: Service<'_>,
    name: String,
) -> Result<ProjectAgent> {
    // With no server there is no project agent: the page still opens, on an id the name makes,
    // with no chat.
    if beep.server().is_none() {
        let found = desk(&fleet).await?.project_machines(&name).await;
        return found
            .map(|_| ProjectAgent {
                project_id: project_id(&name),
                name,
                chat_id: Uuid::nil(),
                open_tasks: 0,
                open_workers: 0,
            })
            .map_err(Into::into);
    }
    beep.call(GetProjectAgent { project: name })
        .await
        .map_err(outdated)
}

/// One task as a project's page lists it.
#[derive(Debug, Serialize)]
pub struct TaskRow {
    id: Uuid,
    title: String,
    state: &'static str,
    /// The task that must finish first.
    depends_on: Option<Uuid>,
    issue_number: Option<u64>,
    issue_url: Option<String>,
    branch: Option<String>,
    pr_url: Option<String>,
    /// The worker on it now, if one is.
    worker: Option<Uuid>,
    /// In milliseconds since the Unix epoch.
    updated_at: u64,
}

/// One worker as a project's page lists it.
#[derive(Debug, Serialize)]
pub struct WorkerRow {
    id: Uuid,
    task_id: Uuid,
    /// The `Machine::id` it runs on.
    machine_id: String,
    machine_name: String,
    agent: String,
    /// The node's id for its coding-agent session, once it has started.
    session: Option<String>,
    folder: String,
    state: &'static str,
    /// Not ended or failed, so it can be stopped.
    live: bool,
    last_output: Option<String>,
    persona: Option<String>,
    /// In milliseconds since the Unix epoch.
    started_at: u64,
    ended_at: Option<u64>,
}

/// A project's page below its folders: the agent's tasks, its workers,
/// its chat and the approvals that chat waits on.
#[derive(Debug, Serialize)]
pub struct ProjectView {
    tasks: Vec<TaskRow>,
    workers: Vec<WorkerRow>,
    turns: Vec<Turn>,
    cards: Vec<PendingApproval>,
}

/// The server's answers for one project's page.
struct ProjectParts {
    chat_id: Uuid,
    tasks: Vec<ProjectTask>,
    workers: Vec<ProjectWorker>,
    turns: Vec<Turn>,
    approvals: Vec<ChatApproval>,
}

/// The tasks, workers, chat and cards of the project agent `project_id`,
/// whose chat is `chat_id`.
#[tauri::command]
pub async fn center_project(
    beep: Service<'_>,
    project_id: Uuid,
    chat_id: Uuid,
) -> Result<ProjectView> {
    // No server, no project agent: nothing planned, nothing running, no chat.
    if beep.server().is_none() {
        return Ok(project_view(ProjectParts {
            chat_id,
            tasks: Vec::new(),
            workers: Vec::new(),
            turns: Vec::new(),
            approvals: Vec::new(),
        }));
    }
    let (tasks, workers, turns, approvals) = join!(
        beep.call(ListProjectTasks {
            project: project_id
        }),
        beep.call(ListProjectWorkers {
            project: project_id
        }),
        beep.history(chat_id.to_string()),
        beep.call(ListPendingApprovals {}),
    );
    Ok(project_view(ProjectParts {
        chat_id,
        tasks: tasks?,
        workers: workers?,
        turns: turns?,
        approvals: approvals?,
    }))
}

/// Sends `text` to the project chat `chat_id`, as the bar sends a prompt,
/// and returns once the agent's turn ends.
#[tauri::command]
pub async fn center_project_send(
    window: WebviewWindow,
    beep: Service<'_>,
    chat_id: Uuid,
    text: String,
    on_event: Channel<BarEvent>,
) -> Result<()> {
    let sent = Sent {
        message: text,
        images: Vec::new(),
        files: Vec::new(),
        research: false,
        voice: None,
        spoken: false,
    };
    // Made current before the run, so its own notices stay quiet.
    center::remember_for(&window, &beep, Chat::Session(Some(chat_id.to_string())));
    beep.post(chat_id, sent, Pick::default(), on_event).await?;
    Ok(())
}

/// What the user may do to a task from its row.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskChange {
    Drop,
    Reopen,
}

impl From<TaskChange> for ProjectTaskState {
    fn from(change: TaskChange) -> Self {
        match change {
            TaskChange::Drop => Self::Dropped {},
            TaskChange::Reopen => Self::Planned {},
        }
    }
}

/// Drops a task, or reopens a dropped or finished one.
#[tauri::command]
pub async fn center_update_task(beep: Service<'_>, task: Uuid, change: TaskChange) -> Result<()> {
    beep.call(UpdateProjectTask {
        task,
        state: change.into(),
    })
    .await
    .map_err(outdated)?;
    Ok(())
}

/// Stops a live worker.
#[tauri::command]
pub async fn center_stop_worker(beep: Service<'_>, worker: Uuid) -> Result<()> {
    beep.call(StopProjectWorker { worker })
        .await
        .map_err(outdated)
}

/// Where a task sorts: work in hand first, then what waits, then what is
/// over.
fn task_rank(state: ProjectTaskState) -> u8 {
    match state {
        ProjectTaskState::Running {} => 0,
        ProjectTaskState::Review {} => 1,
        ProjectTaskState::Blocked {} => 2,
        ProjectTaskState::Planned {} => 3,
        ProjectTaskState::Done {} => 4,
        ProjectTaskState::Dropped {} => 5,
    }
}

/// Whether a worker still runs, or may.
fn is_live(state: ProjectWorkerState) -> bool {
    !matches!(
        state,
        ProjectWorkerState::Ended {} | ProjectWorkerState::Failed {}
    )
}

/// The page from the server's answers: tasks by state, then oldest first,
/// each with its live worker; live workers before ended ones, each newest
/// first; and only the cards of this project's chat.
fn project_view(parts: ProjectParts) -> ProjectView {
    let ProjectParts {
        chat_id,
        mut tasks,
        mut workers,
        turns,
        approvals,
    } = parts;
    tasks.sort_by_key(|t| (task_rank(t.state), t.created_at));
    workers.sort_by_key(|w| (!is_live(w.state), Reverse(w.started_at)));
    let tasks = tasks
        .into_iter()
        .map(|t| TaskRow {
            worker: workers
                .iter()
                .find(|w| w.task_id == t.id && is_live(w.state))
                .map(|w| w.id),
            id: t.id,
            title: t.title,
            state: t.state.into(),
            depends_on: t.depends_on,
            issue_number: t.issue_number,
            issue_url: t.issue_url,
            branch: t.branch,
            pr_url: t.pr_url,
            updated_at: epoch_ms(t.updated_at),
        })
        .collect();
    let workers = workers
        .into_iter()
        .map(|w| WorkerRow {
            live: is_live(w.state),
            id: w.id,
            task_id: w.task_id,
            machine_id: w.machine_id,
            machine_name: w.machine_name,
            agent: w.agent,
            session: w.session,
            folder: w.folder,
            state: w.state.into(),
            last_output: w.last_output,
            persona: w.persona,
            started_at: epoch_ms(w.started_at),
            ended_at: w.ended_at.map(epoch_ms),
        })
        .collect();
    let cards = approvals
        .into_iter()
        .filter(|a| a.session_id == chat_id)
        .map(|a| a.pending)
        .collect();
    ProjectView {
        tasks,
        workers,
        turns,
        cards,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::Approval;
    use std::time::{Duration, SystemTime};

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn task(title: &str, state: ProjectTaskState, created: u64) -> ProjectTask {
        ProjectTask {
            id: Uuid::new_v4(),
            slug: title.replace(' ', "-"),
            title: title.into(),
            body: String::new(),
            state,
            depends_on: None,
            issue_number: None,
            issue_url: None,
            branch: None,
            pr_url: None,
            created_at: at(created),
            updated_at: at(created),
        }
    }

    fn worker(task_id: Uuid, state: ProjectWorkerState, started: u64) -> ProjectWorker {
        ProjectWorker {
            id: Uuid::new_v4(),
            task_id,
            machine_id: "m1".into(),
            machine_name: "build-box".into(),
            agent: "claude".into(),
            session: None,
            folder: "/srv/project/.worktrees/task".into(),
            state,
            last_output: None,
            persona: None,
            started_at: at(started),
            ended_at: None,
        }
    }

    fn approval(chat: Uuid, query: &str) -> ChatApproval {
        ChatApproval {
            session_id: chat,
            session_title: "acme/widget".into(),
            pending: PendingApproval {
                run_id: "run".into(),
                query_id: query.into(),
                approval: Approval {
                    app: "Project".into(),
                    action: "Create 3 tasks".into(),
                    fields: Vec::new(),
                },
            },
        }
    }

    fn parts() -> ProjectParts {
        ProjectParts {
            chat_id: Uuid::new_v4(),
            tasks: Vec::new(),
            workers: Vec::new(),
            turns: Vec::new(),
            approvals: Vec::new(),
        }
    }

    #[test]
    fn tasks_order_by_state() {
        let view = project_view(ProjectParts {
            tasks: vec![
                task("dropped", ProjectTaskState::Dropped {}, 1),
                task("done", ProjectTaskState::Done {}, 2),
                task("planned late", ProjectTaskState::Planned {}, 9),
                task("planned early", ProjectTaskState::Planned {}, 3),
                task("blocked", ProjectTaskState::Blocked {}, 4),
                task("review", ProjectTaskState::Review {}, 5),
                task("running", ProjectTaskState::Running {}, 6),
            ],
            ..parts()
        });
        let titles: Vec<&str> = view.tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "running",
                "review",
                "blocked",
                "planned early",
                "planned late",
                "done",
                "dropped"
            ]
        );
        assert_eq!(view.tasks[0].state, "running");
    }

    #[test]
    fn workers_live_first_then_newest() {
        let t = Uuid::new_v4();
        let view = project_view(ProjectParts {
            workers: vec![
                worker(t, ProjectWorkerState::Ended {}, 50),
                worker(t, ProjectWorkerState::Running {}, 10),
                worker(t, ProjectWorkerState::Failed {}, 60),
                worker(t, ProjectWorkerState::Waiting {}, 20),
            ],
            ..parts()
        });
        let order: Vec<(u64, bool)> = view
            .workers
            .iter()
            .map(|w| (w.started_at / 1000, w.live))
            .collect();
        assert_eq!(order, [(20, true), (10, true), (60, false), (50, false)]);
    }

    #[test]
    fn cards_only_for_this_chat() {
        let base = parts();
        let chat = base.chat_id;
        let view = project_view(ProjectParts {
            approvals: vec![
                approval(Uuid::new_v4(), "elsewhere"),
                approval(chat, "plan"),
            ],
            ..base
        });
        let queries: Vec<&str> = view.cards.iter().map(|c| c.query_id.as_str()).collect();
        assert_eq!(queries, ["plan"]);
    }

    #[test]
    fn task_rows_link_their_live_worker() {
        let busy = task("busy", ProjectTaskState::Running {}, 1);
        let idle = task("idle", ProjectTaskState::Blocked {}, 2);
        let ended = worker(busy.id, ProjectWorkerState::Ended {}, 5);
        let live = worker(busy.id, ProjectWorkerState::Running {}, 9);
        let gone = worker(idle.id, ProjectWorkerState::Failed {}, 7);
        let live_id = live.id;
        let view = project_view(ProjectParts {
            tasks: vec![idle, busy],
            workers: vec![ended, gone, live],
            ..parts()
        });
        assert_eq!(view.tasks[0].title, "busy");
        assert_eq!(view.tasks[0].worker, Some(live_id));
        assert_eq!(view.tasks[1].worker, None);
    }
}
