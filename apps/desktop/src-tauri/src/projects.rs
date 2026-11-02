//! Projects imported onto the user's machines: the Import a project sheet,
//! its folder choice, the rows the sidebar, Home and the Projects page
//! list, and a project agent's tasks, workers and chat on a project's page.

use crate::{
    agents::SessionRow,
    beep::{BarEvent, Beep, Pick, Sent, Turn, epoch_ms},
    center::{self, Chat},
    error::{DesktopError, Result},
};
use beep_proto::{
    ApiError, BranchPlace, ChatApproval, FoundAgent, FoundPlace, FoundProject, GetProjectAgent,
    ImportProject, ImportedProject, ListFoundFolders, ListMachineFolder, ListPendingApprovals,
    ListProjectMachines, ListProjectSessions, ListProjectSyncthing, ListProjectTasks,
    ListProjectWorkers, ListProjects, ListProjectsOnTop, MachineFolder, PendingApproval,
    PinProjectToTop, ProjectAgent, ProjectMachine, ProjectTask, ProjectTaskState, ProjectWorker,
    ProjectWorkerState, RemoveProject, StopProjectWorker, SyncthingPlace, UpdateProjectTask,
};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};
use tauri::{AppHandle, State, WebviewWindow, ipc::Channel};
use tauri_plugin_dialog::{DialogExt, FilePath};
use tokio::{join, sync::oneshot};
use uuid::Uuid;

type Service<'a> = State<'a, Arc<Beep>>;

/// What an old server answers a request it does not know.
const UNKNOWN_OP: &str = "unknown op";
/// The server stops a clone after 30 minutes; this leaves it time to say so.
const IMPORT_TIMEOUT: Duration = Duration::from_secs(31 * 60);

/// A branch the import sheet gives a worktree, on `machine_id` or, without
/// one, the machine the project lands on.
#[derive(Debug, Deserialize)]
pub struct WorktreeArg {
    branch: String,
    machine_id: Option<String>,
}

/// One imported project as the window shows it.
#[derive(Debug, Serialize)]
pub struct ProjectRow {
    /// `org/repo`.
    name: String,
    url: String,
    machine_id: String,
    machine_name: String,
    folder: String,
    cloned: bool,
    /// In milliseconds since the Unix epoch.
    imported_at: u64,
}

impl From<ImportedProject> for ProjectRow {
    fn from(p: ImportedProject) -> Self {
        Self {
            name: p.project,
            url: p.url,
            machine_id: p.machine_id,
            machine_name: p.machine_name,
            folder: p.folder,
            cloned: p.cloned,
            imported_at: epoch_ms(p.imported_at),
        }
    }
}

/// A git checkout the user's coding agents worked in, as the Add a project
/// sheet lists it.
#[derive(Debug, Serialize)]
pub struct FoundRow {
    /// `org/repo`.
    name: String,
    /// What `import_project` takes as the repository.
    remote: String,
    places: Vec<FoundPlace>,
    agents: Vec<FoundAgent>,
    /// In milliseconds since the Unix epoch.
    last_activity: u64,
}

impl From<FoundProject> for FoundRow {
    fn from(p: FoundProject) -> Self {
        Self {
            name: p.project,
            remote: p.remote,
            places: p.places,
            agents: p.agents,
            last_activity: epoch_ms(p.last_activity),
        }
    }
}

/// The checkouts the user's coding agents worked in that are not projects
/// yet; none from a server too old to look.
#[tauri::command]
pub async fn found_projects(beep: Service<'_>) -> Result<Vec<FoundRow>> {
    match beep.call(ListFoundFolders {}).await {
        Ok(found) => Ok(found.into_iter().map(FoundRow::from).collect()),
        Err(e) if unknown_op(&e) => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// The sessions coding agents ran in `project`'s folders, newest first;
/// none from a server too old to list them.
#[tauri::command]
pub async fn project_sessions(beep: Service<'_>, project: String) -> Result<Vec<SessionRow>> {
    match beep.call(ListProjectSessions { project }).await {
        Ok(sessions) => Ok(sessions.into_iter().map(SessionRow::from).collect()),
        Err(e) if unknown_op(&e) => Ok(Vec::new()),
        Err(e) => Err(e),
    }
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
    beep: Service<'_>,
    project: String,
) -> Result<Option<Vec<ProjectMachineRow>>> {
    match beep.call(ListProjectMachines { project }).await {
        Ok(machines) => Ok(Some(
            machines.into_iter().map(ProjectMachineRow::from).collect(),
        )),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The machines on which Syncthing keeps `project`'s folder in step. Empty
/// from a server too old to say.
#[tauri::command]
pub async fn project_syncthing(beep: Service<'_>, project: String) -> Result<Vec<SyncthingPlace>> {
    match beep.call(ListProjectSyncthing { project }).await {
        Ok(places) => Ok(places),
        Err(e) if unknown_op(&e) => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Shows a project's folder on this Mac in Finder.
#[tauri::command]
pub fn open_folder(path: PathBuf) -> Result<()> {
    if !openable(&path) {
        return Err(DesktopError::BadRequest(
            "That folder is not on this Mac.".into(),
        ));
    }
    Command::new("open")
        .arg(&path)
        .spawn()
        .map_err(anyhow::Error::from)?;
    Ok(())
}

/// Only an absolute folder that is there opens.
fn openable(path: &Path) -> bool {
    path.is_absolute() && path.is_dir()
}

/// The server did not know the request.
pub(crate) fn unknown_op(e: &DesktopError) -> bool {
    matches!(e, DesktopError::Api(ApiError::BadRequest { message }) if message.starts_with(UNKNOWN_OP))
}

/// Clones `repo` onto `machine_id`, or the machine the server picks, into
/// `folder` when the user chose one, else the machine's projects folder,
/// then gives each of `worktrees` its worktree.
#[tauri::command]
pub async fn import_project(
    beep: Service<'_>,
    repo: String,
    machine_id: Option<String>,
    folder: Option<String>,
    worktrees: Option<Vec<WorktreeArg>>,
) -> Result<ProjectRow> {
    let machine_id = machine_id.filter(|id| !id.is_empty());
    let folder = folder.filter(|folder| !folder.trim().is_empty());
    let worktrees = worktrees
        .unwrap_or_default()
        .into_iter()
        .map(|w| BranchPlace {
            branch: w.branch,
            machine_id: w.machine_id.filter(|id| !id.is_empty()),
        })
        .collect();
    beep.call_within(
        ImportProject {
            repo,
            machine_id,
            folder,
            worktrees,
        },
        IMPORT_TIMEOUT,
    )
    .await
    .map(ProjectRow::from)
    .map_err(outdated)
}

/// One folder on `machine_id`, for the sheet's folder browser and to see
/// whether a folder picked on this Mac is a checkout: `path`, or the
/// machine's workspace, else its home.
#[tauri::command]
pub async fn list_machine_folder(
    beep: Service<'_>,
    machine_id: String,
    path: Option<String>,
) -> Result<MachineFolder> {
    beep.call(ListMachineFolder { machine_id, path })
        .await
        .map_err(outdated)
}

/// The macOS folder picker, opened at `start`; `None` when the user
/// cancels.
#[tauri::command]
pub async fn pick_folder(app: AppHandle, start: Option<String>) -> Result<Option<String>> {
    let (picked, chosen) = oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Choose a folder for the project")
        .set_can_create_directories(true);
    if let Some(start) = start.filter(|start| !start.is_empty()) {
        dialog = dialog.set_directory(PathBuf::from(start));
    }
    dialog.pick_folder(move |folder| {
        // The sheet may have closed meanwhile; then nobody waits for it.
        let _ = picked.send(folder);
    });
    let folder = chosen
        .await
        .map_err(|_| DesktopError::BadRequest("The folder picker closed.".into()))?;
    folder.map(folder_text).transpose()
}

/// A picked folder as the path the server takes.
fn folder_text(folder: FilePath) -> Result<String> {
    let path = folder
        .into_path()
        .map_err(|e| DesktopError::BadRequest(format!("That folder cannot be used: {e}")))?;
    path.into_os_string()
        .into_string()
        .map_err(|_| DesktopError::BadRequest("That folder's name is not valid text.".into()))
}

/// An old server's refusal, in words the sheet shows; any other refusal
/// is the server's own sentence.
fn outdated(e: DesktopError) -> DesktopError {
    match e {
        DesktopError::Api(ApiError::BadRequest { message }) if message.starts_with(UNKNOWN_OP) => {
            DesktopError::BadRequest(
                "The server needs an update before projects can be imported.".into(),
            )
        }
        DesktopError::Api(ApiError::BadRequest { message }) => DesktopError::BadRequest(message),
        other => other,
    }
}

/// Every project agent of the user's; `None` from a server too old to have
/// them, and the page offers no project chat.
#[tauri::command]
pub async fn center_projects(beep: Service<'_>) -> Result<Option<Vec<ProjectAgent>>> {
    match beep.call(ListProjects {}).await {
        Ok(agents) => Ok(Some(agents)),
        Err(e) if center::is_unsupported(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The projects pinned to the top of the sidebar, oldest pin first; none
/// from a server too old to pin them.
#[tauri::command]
pub async fn projects_on_top(beep: Service<'_>) -> Result<Vec<String>> {
    match beep.call(ListProjectsOnTop {}).await {
        Ok(names) => Ok(names),
        Err(e) if unknown_op(&e) => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Pins `project` (`org/repo`) to the top of the sidebar, or unpins it.
/// Not `PinProject`, which keeps a project off sync.
#[tauri::command]
pub async fn pin_project_to_top(beep: Service<'_>, project: String, pinned: bool) -> Result<()> {
    beep.call(PinProjectToTop { project, pinned })
        .await
        .map_err(|e| {
            if unknown_op(&e) {
                DesktopError::BadRequest(
                    "The server needs an update before projects can be pinned.".into(),
                )
            } else {
                e
            }
        })
}

/// Removes `project` (`org/repo`) from Beep: the server forgets it, stops
/// syncing it and its workers, and deletes its chat and tasks. No files are
/// deleted on any machine.
#[tauri::command]
pub async fn remove_project(beep: Service<'_>, project: String) -> Result<()> {
    beep.call(RemoveProject { project }).await.map_err(|e| {
        if unknown_op(&e) {
            DesktopError::BadRequest(
                "The server needs an update before projects can be removed.".into(),
            )
        } else {
            e
        }
    })
}

/// The project agent of `name` (`org/repo`), made with its chat on the
/// first ask.
#[tauri::command]
pub async fn center_open_project(beep: Service<'_>, name: String) -> Result<ProjectAgent> {
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
    machine_name: String,
    agent: String,
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
            machine_name: w.machine_name,
            agent: w.agent,
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
    use std::time::SystemTime;

    #[test]
    fn only_a_folder_that_is_there_opens() {
        let dir = std::env::temp_dir();
        assert!(openable(&dir));
        assert!(!openable(Path::new("relative/folder")));
        assert!(!openable(&dir.join("no-such-folder-for-beep")));
    }

    #[test]
    fn an_old_server_finds_nothing() {
        assert!(unknown_op(&DesktopError::Api(ApiError::bad_request(
            "unknown op 169"
        ))));
        assert!(!unknown_op(&DesktopError::Api(ApiError::bad_request(
            "offline"
        ))));
    }

    #[test]
    fn refusals_read_as_the_servers_sentence() {
        let old = DesktopError::Api(ApiError::bad_request("unknown op 166"));
        assert!(outdated(old).to_string().contains("needs an update"));
        let said = DesktopError::Api(ApiError::bad_request("build-box is offline."));
        assert_eq!(outdated(said).to_string(), "build-box is offline.");
    }

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
