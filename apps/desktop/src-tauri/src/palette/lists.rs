//! What the palette searches. `follow` reads it for the app's whole life,
//! each source apart so a slow one holds up none of the rest, and rebuilds
//! the index on each answer. The server's lists are saved in the views file,
//! so a new launch finds them before the server answers.

use super::index::build_index;
use crate::{
    beep::{Beep, state::LinkState},
    center::{
        home_sessions::AgentSessions,
        section,
        view_cache::{View, ViewCache, cache_view},
    },
    fleet::Fleet,
    machines::list::{self as machine_list, Outside},
};
use beep_proto::{
    Cron, GitItem, GitItemKind, ImportedProject, ListCrons, ListGitItems, ListImportedProjects,
    ListMemories, ListProjectBranches, ListProjects, ListSessions, Machine, MachineSession, Memory,
    ProjectAgent, Session,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State, ipc::Response};
use tokio::{join, select, sync::Notify, task::JoinSet, time::sleep};

/// Tells the windows to read the index again.
const PALETTE_CHANGED: &str = "palette-changed";
/// The lists are read again this often, and whenever the palette opens.
const READ_EVERY: Duration = Duration::from_secs(300);
/// The least time between two reads of the lists, however often it opens.
const READ_GAP: Duration = Duration::from_secs(10);
/// The least time between two reads of the chats, while one is busy.
const CHATS_GAP: Duration = Duration::from_secs(2);
/// How often to look again while the app is not linked.
const UNLINKED_POLL: Duration = Duration::from_secs(5);
/// The most projects whose branches are read.
const BRANCH_PROJECTS: usize = 8;

/// The lists the index is built from. A list stays `None` until it was read
/// once; after that the last answer stays when a read fails.
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct PaletteLists {
    pub(super) memories: Option<Vec<Memory>>,
    pub(super) routines: Option<Vec<Cron>>,
    pub(super) machines: Option<Vec<Machine>>,
    pub(super) projects: Option<Vec<ImportedProject>>,
    /// The projects board, which also holds projects never imported.
    pub(super) boards: Option<Vec<ProjectAgent>>,
    /// The machines' coding-agent sessions.
    pub(super) sessions: Option<Vec<MachineSession>>,
    pub(super) prs: Option<Vec<PrRow>>,
    pub(super) branches: Option<Vec<BranchRow>>,
    /// From the local copy of the chats, which has its own file.
    #[serde(skip)]
    pub(super) chats: Option<Vec<Session>>,
}

/// A branch with commits its project's default branch does not have.
#[derive(Serialize, Deserialize)]
pub(crate) struct BranchRow {
    pub(super) project: String,
    pub(super) name: String,
    pub(super) ahead: u32,
    pub(super) subject: String,
}

/// An open pull request of the user's, as the palette finds it.
#[derive(Serialize, Deserialize)]
pub(crate) struct PrRow {
    pub(super) repo: String,
    pub(super) number: u64,
    pub(super) title: String,
    pub(super) url: String,
    /// Why it is the user's, such as "PR · asks for your review".
    pub(super) detail: String,
}

impl From<GitItem> for PrRow {
    fn from(item: GitItem) -> Self {
        let why = if item.review_requested {
            "asks for your review"
        } else if item.authored {
            "yours"
        } else {
            "assigned to you"
        };
        let draft = if item.draft { ", draft" } else { "" };
        Self {
            detail: format!("PR · {why}{draft} · {}", item.host),
            repo: item.repo,
            number: item.number,
            title: item.title,
            url: item.url,
        }
    }
}

struct Held {
    lists: PaletteLists,
    /// The index as JSON, as the windows read it.
    index: String,
}

/// The lists and their index.
pub(crate) struct PaletteCache {
    held: Mutex<Held>,
    read_lists: Notify,
    read_chats: Notify,
}

fn index_json(lists: &PaletteLists) -> anyhow::Result<String> {
    Ok(serde_json::to_string(&build_index(lists))?)
}

impl PaletteCache {
    /// Starts from the lists saved in `views`, if it holds them.
    pub(crate) fn load(views: &ViewCache) -> anyhow::Result<Self> {
        let lists = match views.get(View::Palette).map(serde_json::from_value) {
            Some(Ok(lists)) => lists,
            Some(Err(e)) => {
                // Only a head start: the server's lists replace it.
                log::info!("the saved palette lists do not parse: {e}");
                PaletteLists::default()
            }
            None => PaletteLists::default(),
        };
        let index = index_json(&lists)?;
        Ok(Self {
            held: Mutex::new(Held { lists, index }),
            read_lists: Notify::new(),
            read_chats: Notify::new(),
        })
    }

    fn held(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The chats in the local copy changed: read them for the index, soon.
    pub(crate) fn chats_changed(&self) {
        self.read_chats.notify_one();
    }

    /// Changes the lists and rebuilds the index. The windows hear of it only
    /// when the index came out different.
    fn update(&self, app: &AppHandle, change: impl FnOnce(&mut PaletteLists)) {
        let changed = {
            let mut held = self.held();
            change(&mut held.lists);
            match index_json(&held.lists) {
                Ok(index) if index != held.index => {
                    held.index = index;
                    true
                }
                Ok(_) => false,
                Err(e) => {
                    log::warn!("writing the palette's index: {e}");
                    false
                }
            }
        };
        if changed && let Err(e) = app.emit(PALETTE_CHANGED, ()) {
            log::warn!("telling the windows the palette changed: {e}");
        }
    }
}

/// The index as JSON.
#[tauri::command]
pub fn center_palette_index(cache: State<'_, PaletteCache>) -> Response {
    Response::new(cache.held().index.clone())
}

/// The palette opened: read the lists again, without anyone waiting for it.
#[tauri::command]
pub fn center_palette_refresh(cache: State<'_, PaletteCache>) {
    cache.read_lists.notify_one();
}

/// Keeps the index current for the app's life.
pub(crate) async fn follow(app: AppHandle, beep: Arc<Beep>) {
    join!(follow_lists(&app, &beep), follow_chats(&app, &beep));
}

async fn follow_lists(app: &AppHandle, beep: &Arc<Beep>) {
    let cache = app.state::<PaletteCache>();
    loop {
        if beep.link_state() != LinkState::Linked {
            sleep(UNLINKED_POLL).await;
            continue;
        }
        read_lists(app, beep).await;
        sleep(READ_GAP).await;
        select! {
            () = sleep(READ_EVERY) => {}
            () = cache.read_lists.notified() => {}
        }
    }
}

async fn follow_chats(app: &AppHandle, beep: &Arc<Beep>) {
    let cache = app.state::<PaletteCache>();
    loop {
        // Walks every chat's messages, so off the async workers.
        let reader = Arc::clone(beep);
        let copy = tauri::async_runtime::spawn_blocking(move || reader.chats().sessions()).await;
        let chats = match copy {
            Ok(Some(chats)) => Some(chats),
            Ok(None) if beep.link_state() == LinkState::Linked => {
                section("chats", beep.call(ListSessions {}).await)
            }
            Ok(None) => None,
            Err(e) => {
                log::warn!("reading the chats for the palette: {e}");
                None
            }
        };
        if chats.is_some() {
            cache.update(app, |lists| lists.chats = chats);
        }
        sleep(CHATS_GAP).await;
        cache.read_chats.notified().await;
    }
}

/// Reads every list, then saves them for the next launch.
async fn read_lists(app: &AppHandle, beep: &Arc<Beep>) {
    let cache = app.state::<PaletteCache>();
    let views = app.state::<Arc<ViewCache>>();
    join!(
        async {
            read_server_lists(app, beep).await;
            // Branches are read for the projects just listed.
            read_branches(app, beep).await;
        },
        read_prs(app, beep),
        read_sessions(app, beep),
    );
    cache_view(&views, View::Palette, &cache.held().lists);
}

async fn read_server_lists(app: &AppHandle, beep: &Beep) {
    let (memories, routines, projects, boards) = join!(
        beep.call(ListMemories {}),
        beep.call(ListCrons {}),
        beep.call(ListImportedProjects {}),
        beep.call(ListProjects {}),
    );
    let memories = section("memories", memories);
    let routines = section("routines", routines);
    let machines = Some(machine_list::machines(
        app,
        &app.state::<Arc<Fleet>>(),
        &app.state::<Outside>(),
    ));
    let projects = section("projects", projects);
    let boards = section("the projects board", boards);
    app.state::<PaletteCache>().update(app, |lists| {
        lists.memories = memories.or(lists.memories.take());
        lists.routines = routines.or(lists.routines.take());
        lists.machines = machines.or(lists.machines.take());
        lists.projects = projects.or(lists.projects.take());
        lists.boards = boards.or(lists.boards.take());
    });
}

/// The server may ask the git hosts.
async fn read_prs(app: &AppHandle, beep: &Beep) {
    let Some(git) = section("pull requests", beep.call(ListGitItems {}).await) else {
        return;
    };
    let prs = git
        .items
        .into_iter()
        .filter(|item| item.kind == (GitItemKind::PullRequest {}))
        .map(PrRow::from)
        .collect();
    app.state::<PaletteCache>()
        .update(app, |lists| lists.prs = Some(prs));
}

/// The server asks every machine.
async fn read_sessions(app: &AppHandle, beep: &Arc<Beep>) {
    let agents = app.state::<Arc<AgentSessions>>();
    let views = app.state::<Arc<ViewCache>>();
    let listed = agents.list(app, beep, &views).await;
    let Some(sessions) = section("agent sessions", listed) else {
        return;
    };
    app.state::<PaletteCache>()
        .update(app, |lists| lists.sessions = Some(sessions));
}

/// Each project's are read on a machine.
async fn read_branches(app: &AppHandle, beep: &Arc<Beep>) {
    let cache = app.state::<PaletteCache>();
    let projects: Vec<String> = {
        let held = cache.held();
        let mut seen = HashSet::new();
        held.lists
            .projects
            .iter()
            .flatten()
            .filter(|p| seen.insert(p.project.clone()))
            .map(|p| p.project.clone())
            .take(BRANCH_PROJECTS)
            .collect()
    };
    let mut reads = JoinSet::new();
    for project in projects {
        let beep = Arc::clone(beep);
        reads.spawn(async move {
            let read = beep
                .call(ListProjectBranches {
                    project: project.clone(),
                })
                .await;
            (project, read)
        });
    }
    let mut rows = Vec::new();
    while let Some(done) = reads.join_next().await {
        let (project, read) = match done {
            Ok(done) => done,
            Err(e) => {
                log::warn!("reading branches for the palette: {e}");
                continue;
            }
        };
        let Some(found) = section("branches", read) else {
            continue;
        };
        rows.extend(
            found
                .branches
                .into_iter()
                .filter(|b| b.ahead > 0 && b.name != found.default_branch)
                .map(|b| BranchRow {
                    project: project.clone(),
                    name: b.name,
                    ahead: b.ahead,
                    subject: b.subject,
                }),
        );
    }
    rows.sort_by(|a, b| (&a.project, &a.name).cmp(&(&b.project, &b.name)));
    cache.update(app, |lists| lists.branches = Some(rows));
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::MemorySource;
    use std::time::SystemTime;

    /// A list that does not survive the file would leave the next launch
    /// empty, and only log it.
    #[test]
    fn lists_come_back_from_the_views_file() {
        let lists = PaletteLists {
            memories: Some(vec![Memory {
                id: "m".to_owned(),
                content: "likes tea".to_owned(),
                category: "facts".to_owned(),
                source: MemorySource::User {},
                updated: "2026-01-01".to_owned(),
                valid_from: None,
                valid_until: None,
                quote: None,
            }]),
            machines: Some(vec![Machine {
                id: "b".to_owned(),
                name: "build-box".to_owned(),
                online: true,
                seen_at: SystemTime::UNIX_EPOCH,
                version: String::new(),
                env: None,
            }]),
            chats: Some(Vec::new()),
            ..PaletteLists::default()
        };
        let saved = serde_json::to_value(&lists).expect("the lists are plain data");
        let back: PaletteLists = serde_json::from_value(saved).expect("they read back");
        assert_eq!(back.memories.expect("kept")[0].content, "likes tea");
        assert_eq!(back.machines.expect("kept")[0].name, "build-box");
        assert!(back.chats.is_none(), "chats have their own file");
    }
}
