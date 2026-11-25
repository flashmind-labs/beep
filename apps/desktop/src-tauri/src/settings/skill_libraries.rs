//! Skill libraries: packs of agent skills from a Git repository, installed globally or in
//! projects. The desktop asks a node to read a library's repository, then writes the library to the
//! mesh; every node installs the picked skills.

use crate::{
    error::{DesktopError, Result},
    fleet::{
        Fleet,
        projects::{FleetState, desk},
    },
};
use beep_fleet::{Desk, Session, library_name};
use beep_proto::node::{
    mesh_state::{Pick, ProjectSkills, SkillLibrary},
    skill_library::{PROTOCOL, Problem, Reply, Request},
};
use futures::{StreamExt, stream::FuturesUnordered};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::State;

/// How long after a failed check the libraries are not checked again, whatever changes.
const RETRY_AFTER: Duration = Duration::from_secs(5 * 60);

/// When a failed check may run again.
#[derive(Default)]
struct Retry(Option<Instant>);

impl Retry {
    fn failed(&mut self, now: Instant) {
        self.0 = Some(now + RETRY_AFTER);
    }

    /// How long until the check may run again; `None` when it may now.
    fn left(&mut self, now: Instant) -> Option<Duration> {
        match self.0.map(|at| at.saturating_duration_since(now)) {
            Some(left) if !left.is_zero() => Some(left),
            _ => {
                self.0 = None;
                None
            }
        }
    }
}

/// Checks the libraries when the app starts, when a machine comes or goes (a controller that
/// connected, or one that failed before, may be the one to check), and when the soonest one is
/// due again. Nothing wakes it while every library is due and unchecked here: a change does.
/// A check that failed is not repeated for `RETRY_AFTER`, so a library that always fails does
/// not keep a busy mesh reading it.
pub(crate) async fn follow(fleet: Arc<Fleet>, session: Arc<Session>) {
    let mut changes = fleet.changes();
    let mut retry = Retry::default();

    loop {
        let mut due = retry.left(Instant::now());

        if due.is_none()
            && let Ok(desk) = session.desk().await
        {
            if let Err(error) = desk.check_skill_libraries().await {
                log::warn!("could not check skill libraries: {error}");
                retry.failed(Instant::now());
                due = retry.left(Instant::now());
            } else {
                due = desk.next_library_check_in().filter(|wait| !wait.is_zero());
            }
        }

        tokio::select! {
            more = changes.changed() => {
                if !more {
                    return;
                }
            }
            () = async {
                match due {
                    Some(wait) => tokio::time::sleep(wait).await,
                    None => std::future::pending().await,
                }
            } => {}
        }
    }
}

/// A library as Settings lists it: its name, then the mesh record.
#[derive(Debug, Serialize)]
pub(crate) struct Library {
    name: String,
    #[serde(flatten)]
    library: SkillLibrary,
}

/// The libraries and how a project keeps its installed skills.
#[derive(Debug, Serialize)]
pub(crate) struct ProjectLibraries {
    libraries: Vec<Library>,
    keep_out_of_git: bool,
}

/// Skills on the machines that come from one Git repository and are no library yet.
#[derive(Debug, Serialize)]
pub(crate) struct Found {
    url: String,
    /// The name the library would get.
    name: String,
    skills: BTreeSet<String>,
    places: Vec<FoundPlace>,
}

/// A skills folder on a machine that holds a found library's skills.
#[derive(Debug, Serialize)]
pub(crate) struct FoundPlace {
    machine: String,
    folder: String,
}

/// What a machine could not fetch or install.
#[derive(Debug, Serialize)]
pub(crate) struct MachineProblem {
    machine: String,
    #[serde(flatten)]
    problem: Problem,
}

#[tauri::command]
pub async fn skill_libraries_list(session: FleetState<'_>) -> Result<Vec<Library>> {
    Ok(libraries(&desk(&session).await?))
}

/// Has a node clone `url` and adds it as a library, installed in every agent's skills folder
/// when `global`, else nowhere.
#[tauri::command]
pub async fn skill_library_add(
    session: FleetState<'_>,
    url: String,
    global: bool,
) -> Result<Library> {
    let (name, library) = desk(&session)
        .await?
        .add_skill_library(url.trim().to_owned(), global)
        .await?;
    Ok(Library { name, library })
}

/// Reads the library's repository again; every machine moves to the new revision, picks kept.
#[tauri::command]
pub async fn skill_library_update(session: FleetState<'_>, name: String) -> Result<()> {
    desk(&session).await?.update_skill_library(&name).await?;
    Ok(())
}

#[tauri::command]
pub async fn skill_library_remove(session: FleetState<'_>, name: String) -> Result<()> {
    desk(&session).await?.remove_skill_library(&name).await?;
    Ok(())
}

#[tauri::command]
pub async fn skill_library_set_global(
    session: FleetState<'_>,
    name: String,
    pick: Pick,
) -> Result<()> {
    desk(&session)
        .await?
        .edit_skill_library(&name, |library| library.global = pick)
        .await?;
    Ok(())
}

/// Installs `skill` in every agent's skills folder, or uninstalls it from there and from every
/// project and chat.
#[tauri::command]
pub async fn skill_library_set_skill(
    session: FleetState<'_>,
    name: String,
    skill: String,
    installed: bool,
) -> Result<()> {
    desk(&session)
        .await?
        .edit_skill_library(&name, |library| {
            if installed {
                library.global = with_skill(&library.global, &skill);
                return;
            }

            let all: Vec<&str> = library.skills.iter().map(|s| s.name.as_str()).collect();
            library.global = without_skill(&library.global, &skill, &all);
            for picks in [&mut library.projects, &mut library.chats] {
                for pick in picks.values_mut() {
                    *pick = without_skill(pick, &skill, &all);
                }
                picks.retain(|_, pick| *pick != Pick::None);
            }
        })
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn skill_library_uninstall_projects(session: FleetState<'_>, name: String) -> Result<()> {
    desk(&session)
        .await?
        .edit_skill_library(&name, |library| library.projects.clear())
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn project_skills_get(
    session: FleetState<'_>,
    project: String,
) -> Result<ProjectLibraries> {
    let desk = desk(&session).await?;
    Ok(ProjectLibraries {
        libraries: libraries(&desk),
        keep_out_of_git: desk.project_skills(&project).keep_out_of_git,
    })
}

/// Saves which skills of each library `project` gets, and whether Git sees them.
#[tauri::command]
pub async fn project_skills_set(
    session: FleetState<'_>,
    project: String,
    picks: BTreeMap<String, Pick>,
    keep_out_of_git: bool,
) -> Result<()> {
    let desk = desk(&session).await?;
    set_picks(&desk, picks, &project, |library| &mut library.projects).await?;

    let settings = ProjectSkills { keep_out_of_git };
    if desk.project_skills(&project) != settings {
        desk.set_project_skills(&project, &settings).await?;
    }
    Ok(())
}

/// Saves which skills of each library the assistant chat whose folder is named `chat` gets, in
/// its folder and its threads'.
#[tauri::command]
pub async fn chat_skills_set(
    session: FleetState<'_>,
    chat: String,
    picks: BTreeMap<String, Pick>,
) -> Result<()> {
    let desk = desk(&session).await?;
    set_picks(&desk, picks, &chat, |library| &mut library.chats).await
}

/// Writes each library whose pick for `place`, in the map `of` gives, is not the one in `picks`.
async fn set_picks(
    desk: &Desk,
    picks: BTreeMap<String, Pick>,
    place: &str,
    of: fn(&mut SkillLibrary) -> &mut BTreeMap<String, Pick>,
) -> Result<()> {
    for (name, pick) in picks {
        let Some(mut library) = desk.skill_library(&name) else {
            continue;
        };

        if of(&mut library).get(place).unwrap_or(&Pick::None) == &pick {
            continue;
        }

        desk.edit_skill_library(&name, |library| match pick {
            Pick::None => {
                of(library).remove(place);
            }
            pick => {
                of(library).insert(place.to_owned(), pick);
            }
        })
        .await?;
    }

    Ok(())
}

/// Libraries already in the agents' skills folders on the online machines, by repository,
/// leaving out the ones that are libraries.
#[tauri::command]
pub async fn skill_libraries_find(
    session: FleetState<'_>,
    fleet: State<'_, Arc<Fleet>>,
) -> Result<Vec<Found>> {
    let desk = desk(&session).await?;
    let known: BTreeSet<String> = libraries(&desk)
        .into_iter()
        .map(|library| same_repo(&library.library.url))
        .collect();
    let mut found: BTreeMap<String, Found> = BTreeMap::new();

    for (machine, reply) in ask_online(&fleet, Request::Find {}).await {
        let Reply::Found { libraries } = reply else {
            continue;
        };

        for library in libraries {
            let key = same_repo(&library.url);
            if known.contains(&key) {
                continue;
            }

            let entry = found.entry(key).or_insert_with(|| Found {
                name: library_name(&library.url),
                url: library.url,
                skills: BTreeSet::new(),
                places: Vec::new(),
            });
            entry.skills.extend(library.skills);
            entry
                .places
                .extend(library.folders.into_iter().map(|folder| FoundPlace {
                    machine: machine.clone(),
                    folder,
                }));
        }
    }

    Ok(found.into_values().collect())
}

/// What each online machine could not fetch or install.
#[tauri::command]
pub async fn skill_libraries_problems(fleet: State<'_, Arc<Fleet>>) -> Result<Vec<MachineProblem>> {
    let mut problems = Vec::new();

    for (machine, reply) in ask_online(&fleet, Request::Problems {}).await {
        if let Reply::Problems { problems: found } = reply {
            problems.extend(found.into_iter().map(|problem| MachineProblem {
                machine: machine.clone(),
                problem,
            }));
        }
    }

    Ok(problems)
}

fn libraries(desk: &Desk) -> Vec<Library> {
    desk.skill_libraries()
        .into_iter()
        .map(|(name, library)| Library { name, library })
        .collect()
}

/// `request` to every online machine at once, with each answer and the machine's name. Machines
/// that fail or run an older node are left out.
async fn ask_online(fleet: &Fleet, request: Request) -> Vec<(String, Reply)> {
    let request = &request;
    let mut calls: FuturesUnordered<_> = fleet
        .online()
        .into_iter()
        .map(|machine| async move {
            let reply = async {
                fleet
                    .client(machine)
                    .await?
                    .call_within::<_, Reply>(Duration::from_secs(5), PROTOCOL, request)
                    .await
                    .map_err(DesktopError::from)
            }
            .await;
            (machine, reply)
        })
        .collect();
    let mut replies = Vec::new();

    while let Some((machine, reply)) = calls.next().await {
        match reply {
            Ok(reply) => replies.push((fleet.name_of(machine), reply)),
            Err(e) => log::debug!("skill libraries on {}: {e}", machine.fmt_short()),
        }
    }

    replies
}

/// A repository URL compared loosely: no trailing `/` or `.git`, lower case.
fn same_repo(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    url.strip_suffix(".git").unwrap_or(url).to_lowercase()
}

/// `pick` with `skill` added.
fn with_skill(pick: &Pick, skill: &str) -> Pick {
    match pick {
        Pick::All => Pick::All,
        Pick::None => Pick::Only(BTreeSet::from([skill.to_owned()])),
        Pick::Only(names) => {
            let mut names = names.clone();
            names.insert(skill.to_owned());
            Pick::Only(names)
        }
    }
}

/// `pick` without `skill`; `All` turns into every other skill of `all`.
fn without_skill(pick: &Pick, skill: &str, all: &[&str]) -> Pick {
    let names: BTreeSet<String> = match pick {
        Pick::None => return Pick::None,
        Pick::All => all.iter().map(|name| (*name).to_owned()).collect(),
        Pick::Only(names) => names.clone(),
    };
    let rest: BTreeSet<String> = names.into_iter().filter(|name| name != skill).collect();

    if rest.is_empty() {
        Pick::None
    } else {
        Pick::Only(rest)
    }
}

#[cfg(test)]
#[path = "../tests/settings_skill_libraries.rs"]
mod tests;
