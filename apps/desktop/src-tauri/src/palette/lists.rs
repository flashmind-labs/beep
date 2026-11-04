//! What the palette searches, read when it opens and kept in
//! `PaletteCache`: lists from the server first, then pull requests, agent
//! sessions and branches from their own commands, so none holds up the rest.

use crate::{
    beep::Beep,
    center::{home_sessions::AgentSessions, section, view_cache::ViewCache},
    error::Result,
    machines::list::MachineView,
    settings::memories::MemoryView,
};
use beep_proto::{
    Cron, GitItem, GitItemKind, ImportedProject, ListCrons, ListGitItems, ListImportedProjects,
    ListMachines, ListMemories, ListProjectBranches, ListProjects, MachineSession, ProjectAgent,
};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use tauri::{AppHandle, State};
use tokio::{join, task::JoinSet};

type Service<'a> = State<'a, Arc<Beep>>;

/// The lists the palette searches in memory. A list is `None` when the
/// server could not give it, which hides its group.
#[derive(Default)]
pub(crate) struct PaletteLists {
    pub(super) memories: Option<Vec<MemoryView>>,
    pub(super) routines: Option<Vec<Cron>>,
    pub(super) machines: Option<Vec<MachineView>>,
    pub(super) projects: Option<Vec<ImportedProject>>,
    /// The projects board, which also holds projects never imported.
    pub(super) boards: Option<Vec<ProjectAgent>>,
    /// The machines' coding-agent sessions. Read after the rest, since the
    /// server asks every machine.
    pub(super) sessions: Option<Vec<MachineSession>>,
    pub(super) prs: Option<Vec<PrRow>>,
    /// Read after the rest, since each project's are read on a machine.
    pub(super) branches: Option<Vec<BranchRow>>,
}

/// A branch with commits its project's default branch does not have.
pub(crate) struct BranchRow {
    pub(super) project: String,
    pub(super) name: String,
    pub(super) ahead: u32,
    pub(super) subject: String,
}

/// An open pull request of the user's, as the palette finds it.
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

/// The lists `center_palette` fetched last.
#[derive(Default)]
pub(crate) struct PaletteCache(Mutex<PaletteLists>);

impl PaletteCache {
    pub(super) fn lists(&self) -> MutexGuard<'_, PaletteLists> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Fetches what the palette searches, when it opens. A list the server
/// cannot give hides its group. Pull requests, branches and sessions come
/// from their own commands, so the git hosts and machines never hold up
/// these lists.
#[tauri::command]
pub async fn center_palette(beep: Service<'_>, cache: State<'_, PaletteCache>) -> Result<()> {
    let (memories, routines, machines, projects, boards) = join!(
        beep.call(ListMemories {}),
        beep.call(ListCrons {}),
        beep.call(ListMachines {}),
        beep.call(ListImportedProjects {}),
        beep.call(ListProjects {}),
    );
    let mut cached = cache.lists();
    // The last pull requests, branches and sessions stay found until their
    // own commands answer.
    cached.memories =
        section("memories", memories).map(|all| all.into_iter().map(MemoryView::from).collect());
    cached.routines = section("routines", routines);
    cached.machines =
        section("machines", machines).map(|all| all.into_iter().map(MachineView::from).collect());
    cached.projects = section("projects", projects);
    cached.boards = section("the projects board", boards);
    Ok(())
}

/// Reads the user's open pull requests for the palette to find. Called
/// after `center_palette`, without holding up the first search: the
/// server may ask the git hosts.
#[tauri::command]
pub async fn center_palette_prs(beep: Service<'_>, cache: State<'_, PaletteCache>) -> Result<()> {
    let git = beep.call(ListGitItems {}).await;
    let Some(git) = section("pull requests", git) else {
        return Ok(());
    };
    let prs = git
        .items
        .into_iter()
        .filter(|item| item.kind == (GitItemKind::PullRequest {}))
        .map(PrRow::from)
        .collect();
    cache.lists().prs = Some(prs);
    Ok(())
}

/// Reads the machines' coding-agent sessions for the palette to find.
/// Called after `center_palette`, without holding up the first search:
/// the server asks every machine.
#[tauri::command]
pub async fn center_palette_sessions(
    app: AppHandle,
    beep: Service<'_>,
    views: State<'_, Arc<ViewCache>>,
    agents: State<'_, Arc<AgentSessions>>,
    cache: State<'_, PaletteCache>,
) -> Result<()> {
    let listed = agents.list(&app, beep.inner(), views.inner()).await;
    let Some(sessions) = section("agent sessions", listed) else {
        return Ok(());
    };
    cache.lists().sessions = Some(sessions);
    Ok(())
}

/// The most projects whose branches the palette reads when it opens.
const BRANCH_PROJECTS: usize = 8;

/// Reads the branches with work of the projects `center_palette` listed,
/// for the palette to find. Called after it, without holding up the
/// first search.
#[tauri::command]
pub async fn center_palette_branches(
    beep: Service<'_>,
    cache: State<'_, PaletteCache>,
) -> Result<()> {
    let projects: Vec<String> = {
        let lists = cache.lists();
        let mut seen = HashSet::new();
        lists
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
        let beep = Arc::clone(beep.inner());
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
    cache.lists().branches = Some(rows);
    Ok(())
}
