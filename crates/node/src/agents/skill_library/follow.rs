//! The task that keeps every library's copy at its revision and its skills installed where they
//! are picked, and the `skill-library/1` answers. Only the node that read a library's repository
//! clones it; the others take the picked skills' files from a machine that has them.

use super::{
    Libraries, find,
    install::{self, Checkout, Place, Ready, Record},
    online, store,
};
use crate::{
    agents::{
        Agents, conversation, project_folder,
        skill_disk,
    },
    events::Change,
    files::syncthing::expand,
};
use anyhow::{Context, Result, bail};
use beep_mesh::{Key, Mesh, PeerClient};
use beep_proto::watched;
use beep_proto::node::{
    LinkRx, LinkTx,
    mesh_state::{ProjectSkills, SkillLibrary, StateValue},
    skill_library::{PROTOCOL, Problem, Reply, Request},
};
use futures::{StreamExt, stream, stream::FuturesUnordered};
use iroh::EndpointId;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::broadcast::error::{RecvError, TryRecvError},
    task::spawn_blocking,
    time::{Instant, sleep, sleep_until, timeout},
};

/// How long changes settle before a pass, so a desktop's few writes make one pass.
const SETTLE: Duration = Duration::from_millis(300);
/// How soon a pass runs again with nothing changed, while something could not be fetched.
const EVERY: Duration = Duration::from_secs(15 * 60);
/// The most libraries fetched at once.
const FETCHES: usize = 4;
/// How long a peer has to send its request.
const READ_LIMIT: Duration = Duration::from_secs(10);
/// How long a Git fetch of a library may take before the request fails.
const INSPECT_LIMIT: Duration = Duration::from_secs(5 * 60);

/// Installs the libraries' skills now, whenever a library, a project's checkouts or its skill
/// settings change, a session starts in a folder none worked in, a machine comes or goes, and
/// every `EVERY` while a pass had problems, until the mesh stops. Nothing when the host keeps Beep out of the skills
/// folders.
pub(crate) async fn follow(
    mesh: Mesh,
    dir: PathBuf,
    home: Option<PathBuf>,
    libraries: Libraries,
    agents: Arc<Agents>,
) {
    // The same folder each time the task starts over.
    let _ = libraries.shared.dir.set(dir.clone());
    let Some(home) = home else {
        return;
    };
    let mut changes = mesh.changes();
    let mut updates = mesh.updates();
    let mut sessions = agents.events.subscribe();
    let file_dir = dir.clone();
    let mut record = match spawn_blocking(move || Record::load(&file_dir)).await {
        Ok(Ok(record)) => record,
        Ok(Err(e)) => {
            log::warn!("skill libraries are not installed: {e:#}");
            return;
        }
        Err(e) => {
            log::warn!("skill libraries are not installed: {e}");
            return;
        }
    };
    libraries.shared.global.send_replace(Some(record.global_names()));
    let mut said = None;

    loop {
        let now = online(&mesh);
        let folders = agents.sessions.folders();
        let pass = Pass {
            mesh: &mesh,
            dir: &dir,
            home: &home,
            folders: &folders,
        };
        let problems = pass.run(&mut record, &libraries).await;
        let summary = (record.installs.len(), problems.len());

        if said != Some(summary) {
            log::info!(
                "skill libraries: {} installed, {} problems",
                summary.0,
                summary.1
            );
            said = Some(summary);
        }

        // Only a failed fetch is worth trying again with nothing changed.
        let retry = (!problems.is_empty()).then(|| Instant::now() + EVERY);

        loop {
            tokio::select! {
                () = sleep_until(retry.unwrap_or_else(Instant::now)), if retry.is_some() => break,
                change = changes.recv() => match change {
                    Ok(key) if relevant(&key) => break,
                    Err(RecvError::Lagged(_)) => break,
                    Err(RecvError::Closed) => return,
                    Ok(_) => {}
                },
                changed = updates.changed() => {
                    if changed.is_err() {
                        return;
                    }

                    if online(&mesh) != now {
                        break;
                    }
                }
                // A thread of a chat with picks gets them in its folder as it starts.
                change = sessions.recv() => match change {
                    Ok(Change { topic, .. }) if topic.name() == "agents" && agents.sessions.folders() != folders => break,
                    Err(RecvError::Lagged(_)) => break,
                    Err(RecvError::Closed) => sessions = agents.events.subscribe(),
                    Ok(_) => {}
                },
            }
        }

        sleep(SETTLE).await;

        loop {
            match changes.try_recv() {
                Ok(_) | Err(TryRecvError::Lagged(_)) => {}
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Closed) => return,
            }
        }
    }
}

/// Whether a change to `key` can change what is installed here.
fn relevant(key: &Key) -> bool {
    matches!(
        key,
        Key::SkillLibrary(_) | Key::ProjectSkills(_) | Key::Project(_) | Key::Sync(_)
    )
}

/// Every library in the mesh, by name.
fn libraries_of(mesh: &Mesh) -> Vec<(String, SkillLibrary)> {
    mesh.scan("skill-library/")
        .into_iter()
        .filter_map(|(key, bytes)| match key {
            Key::SkillLibrary(name) => SkillLibrary::from_bytes(&bytes)
                .inspect_err(|e| log::debug!("skill library {name} not read: {e}"))
                .ok()
                .map(|library| (name, library)),
            _ => None,
        })
        .collect()
}

/// What one pass reads.
struct Pass<'a> {
    mesh: &'a Mesh,
    dir: &'a Path,
    home: &'a Path,
    /// The folders this node's sessions work in.
    folders: &'a BTreeSet<PathBuf>,
}

impl Pass<'_> {
    /// Fetches every library at its revision, then installs and uninstalls; the record is saved
    /// when it changed. What could not be done.
    async fn run(&self, record: &mut Record, libraries: &Libraries) -> Vec<Problem> {
        let listed = libraries_of(self.mesh);
        let fetched: Vec<(Ready, Option<Problem>)> = stream::iter(listed)
            .map(|(name, library)| self.fetch(name, library))
            .buffer_unordered(FETCHES)
            .collect()
            .await;
        let mut problems = Vec::new();
        let mut ready = Vec::new();

        for (lib, problem) in fetched {
            problems.extend(problem);
            ready.push(lib);
        }

        ready.sort_by(|a, b| a.name.cmp(&b.name));
        let checkouts = self.checkouts(&ready, record);
        let home = self.home.to_owned();
        let mut working = record.clone();
        let applied = spawn_blocking(move || {
            let global = skill_disk::targets(&home);
            let wants = install::wants(&ready, &global, &checkouts);
            let before = working.clone();
            let problems = install::apply(&mut working, wants, &checkouts);
            let saved = if working == before {
                Ok(())
            } else {
                working.save()
            };
            (working, problems, saved)
        })
        .await;

        match applied {
            Ok((working, installing, saved)) => {
                if let Err(e) = saved {
                    log::warn!("skill libraries: the record not written: {e:#}");
                }

                *record = working;
                problems.extend(installing);
            }
            Err(e) => log::warn!("skill libraries: installing stopped: {e}"),
        }

        libraries
            .shared
            .global
            .send_replace(Some(record.global_names()));
        self.tell(libraries, &problems);
        problems
    }

    /// Each project with a library pick or an install, with its checkout here, and each chat
    /// with one, with its folder here and its threads' folders.
    fn checkouts(&self, ready: &[Ready], record: &Record) -> Vec<Checkout> {
        let picked = ready.iter().flat_map(|lib| {
            let projects = lib.library.projects.keys().cloned().map(Place::Project);
            let chats = lib.library.chats.keys().cloned().map(Place::Chat);
            projects.chain(chats)
        });
        let installed = record.installs.values().map(|install| install.place.clone());
        let places: BTreeSet<Place> = picked.chain(installed).collect();

        places
            .into_iter()
            .flat_map(|place| match place {
                Place::Global => Vec::new(),
                Place::Project(project) => self.project_checkout(project).into_iter().collect(),
                Place::Chat(chat) => self.chat_folders(chat),
            })
            .collect()
    }

    /// The checkout of `project` here, if there is one.
    fn project_checkout(&self, project: String) -> Option<Checkout> {
        let folder = project_folder(self.mesh, &project, &self.mesh.me())?;
        let settings: ProjectSkills = self
            .mesh
            .get(&Key::ProjectSkills(project.clone()))
            .and_then(|bytes| ProjectSkills::from_bytes(&bytes).ok())
            .unwrap_or_default();
        let folder = expand(&folder, Some(self.home));

        folder.is_dir().then_some(Checkout {
            place: Place::Project(project),
            folder,
            keep_out_of_git: settings.keep_out_of_git,
        })
    }

    /// The folder of the chat `chat` and each folder its threads work in, or none when the chat
    /// is on another machine. Its folder is no Git checkout, and each harness reads skills only
    /// from the folder it works in, so every thread's gets them too.
    fn chat_folders(&self, chat: String) -> Vec<Checkout> {
        chat_folders(self.home, &chat, self.folders)
            .into_iter()
            .map(|folder| Checkout {
                place: Place::Chat(chat.clone()),
                folder,
                keep_out_of_git: false,
            })
            .collect()
    }

    /// The library's copy at its revision; with no copy and a problem when it could not be
    /// fetched.
    async fn fetch(&self, name: String, library: SkillLibrary) -> (Ready, Option<Problem>) {
        let fetched = if skill_disk::valid_part(&name) {
            self.copy(&library).await
        } else {
            Err(format!("{name:?} is not a library name"))
        };
        let (store, problem) = match fetched {
            Ok(root) => (Some(root), None),
            Err(message) => {
                let problem = Problem {
                    library: name.clone(),
                    skill: None,
                    place: Place::Global.to_string(),
                    message,
                };
                (None, Some(problem))
            }
        };

        (
            Ready {
                name,
                library,
                store,
            },
            problem,
        )
    }

    /// The library's copy at its revision: the clone on the node that read the repository, else
    /// the plain copy with every picked skill, the missing ones taken from the other machines.
    async fn copy(&self, library: &SkillLibrary) -> Result<PathBuf, String> {
        let dir = self.dir.to_owned();
        let url = library.url.clone();
        let revision = library.revision.clone();

        if store::cloned(&dir, &url) {
            return spawn_blocking(move || store::ensure_git(&dir, &url, &revision))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| format!("{e:#}"));
        }

        let root = store::copy_root(&dir, &url, &revision).map_err(|e| format!("{e:#}"))?;
        let missing: Vec<String> = library
            .skills
            .iter()
            .map(|skill| &skill.name)
            .filter(|name| picked(library, name) && !root.join(name).join("SKILL.md").is_file())
            .cloned()
            .collect();
        let peers = online(self.mesh);
        let taken: Vec<Result<(), String>> = stream::iter(missing)
            .map(|skill| self.take(library, &root, skill, &peers))
            .buffer_unordered(FETCHES)
            .collect()
            .await;
        taken.into_iter().collect::<Result<(), String>>()?;
        spawn_blocking(move || store::prune_copies(&dir, &url, &revision))
            .await
            .map_err(|e| e.to_string())?;
        Ok(root)
    }

    /// Writes the library's skill `skill` into the copy at `root`, from the first of `peers`
    /// that has it.
    async fn take(
        &self,
        library: &SkillLibrary,
        root: &Path,
        skill: String,
        peers: &BTreeSet<EndpointId>,
    ) -> Result<(), String> {
        let request = &Request::Files {
            url: library.url.clone(),
            revision: library.revision.clone(),
            skill: skill.clone(),
        };
        let mut asks: FuturesUnordered<_> = peers
            .iter()
            .map(|&peer| async move {
                PeerClient::over(self.mesh.clone(), peer)
                    .call::<_, Reply>(PROTOCOL, request)
                    .await
            })
            .collect();

        while let Some(reply) = asks.next().await {
            let files = match reply {
                Ok(Reply::Files { files }) if !files.is_empty() => files,
                Ok(_) => continue,
                Err(e) => {
                    log::debug!("skill libraries: {skill} not taken: {e}");
                    continue;
                }
            };
            let path = root.join(&skill);

            return spawn_blocking(move || install::write(&path, &files))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| format!("{e:#}"));
        }

        Err(format!(
            "no machine online has {skill}; it is copied from the machine that added the library"
        ))
    }

    /// Logs each problem that is new, and keeps them for `Request::Problems`.
    fn tell(&self, libraries: &Libraries, problems: &[Problem]) {
        let before = watched::read_sent(&libraries.shared.problems, Clone::clone);

        for problem in problems.iter().filter(|problem| !before.contains(problem)) {
            match &problem.skill {
                Some(skill) => log::warn!(
                    "skill library {}: {skill} not installed in {}: {}",
                    problem.library,
                    problem.place,
                    problem.message
                ),
                None => log::warn!(
                    "skill library {} not fetched: {}",
                    problem.library,
                    problem.message
                ),
            }
        }

        libraries.shared.problems.send_replace(problems.to_vec());
    }
}

/// Answers one `skill-library/1` request.
pub(crate) async fn serve(
    mut tx: LinkTx,
    mut rx: LinkRx,
    home: Option<PathBuf>,
    libraries: Libraries,
) -> Result<()> {
    let request: Request = timeout(READ_LIMIT, rx.recv())
        .await
        .context("the peer sent no request")??
        .context("the stream closed before its request")?;
    let dir = libraries
        .shared
        .dir
        .get()
        .cloned()
        .context("skill libraries have not started")?;

    let reply = match request {
        Request::Inspect { url } => inspect(dir, url).await,
        Request::Find {} => {
            let Some(home) = home else {
                bail!("this node keeps out of the skills folders");
            };
            let found = spawn_blocking(move || {
                let ours = Record::load(&dir)
                    .map(|record| record.installs.into_keys().collect())
                    .unwrap_or_default();
                find::find(&home, &ours)
            })
            .await
            .context("finding libraries stopped")?;
            Reply::Found { libraries: found }
        }
        Request::Problems {} => Reply::Problems {
            problems: watched::read_sent(&libraries.shared.problems, Clone::clone),
        },
        Request::Files {
            url,
            revision,
            skill,
        } => {
            let read = spawn_blocking(move || store::skill_at(&dir, &url, &revision, &skill))
                .await
                .context("reading the skill stopped")?;

            match read {
                Ok(Some(files)) => Reply::Files { files },
                _ => Reply::Failed {
                    message: "this machine has no copy of it".to_owned(),
                },
            }
        }
    };
    tx.send(&reply).await?;
    tx.finish().await
}

/// The folder of the chat named `chat` under `home`, then those of `folders` under it, which its
/// threads work in; none when the chat's folder is not here.
fn chat_folders(home: &Path, chat: &str, folders: &BTreeSet<PathBuf>) -> Vec<PathBuf> {
    let Some(folder) = skill_disk::valid_part(chat)
        .then(|| conversation::moved(home, chat))
        .flatten()
        .filter(|folder| folder.is_dir())
    else {
        return Vec::new();
    };
    let threads = folders
        .iter()
        .filter(|thread| thread.starts_with(&folder) && **thread != folder && thread.is_dir())
        .cloned();

    std::iter::once(folder.clone()).chain(threads).collect()
}

/// Whether any place picks the library's skill `name`.
fn picked(library: &SkillLibrary, name: &str) -> bool {
    library.global.has(name) || library.projects.values().chain(library.chats.values()).any(|pick| pick.has(name))
}

/// Fetches the repository at `url` and lists its skills.
async fn inspect(dir: PathBuf, url: String) -> Reply {
    let fetch = spawn_blocking(move || store::inspect_git(&dir, &url));
    let read = match timeout(INSPECT_LIMIT, fetch).await {
        Ok(Ok(read)) => read.map_err(|e| format!("{e:#}")),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("fetching the repository took too long".to_owned()),
    };

    match read {
        Ok((revision, skills)) => Reply::Inspected { revision, skills },
        Err(message) => Reply::Failed { message },
    }
}

#[cfg(test)]
#[path = "../../tests/agents_skill_library_follow.rs"]
mod tests;
