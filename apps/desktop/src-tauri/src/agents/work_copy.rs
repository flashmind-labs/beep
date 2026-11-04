//! Reads of a machine's project folder served from this Mac when sync keeps
//! a copy of the project here. Sync is trusted to carry the files, so nothing
//! is compared with the machine. A project not synced here, or a file sync
//! has not brought yet, goes to the node as if this were not here.

use crate::{beep::Beep, error::Result, projects::unknown_op};
use beep_node::{
    agents::{local_read, local_tree},
    files::{sync::keeps, syncthing::Syncthing},
};
use beep_proto::{
    AgentFileChunk, GetProjectSyncDetail, ImportedProject, ListImportedProjects, ListProjectSync,
    ListProjectSyncthing, ListWorkTree, ProjectSync, SyncthingPlace, WorkTree,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::task::spawn_blocking;

/// How long an answer about one folder stands. Reads come a part at a time,
/// and a pause or a new sync shows within this.
const FRESH: Duration = Duration::from_secs(60);
/// Folders whose answers are kept at once.
const MAX_KEPT: usize = 256;

/// What is known about the folders asked about so far.
#[derive(Default)]
pub struct Copies {
    known: Mutex<HashMap<Asked, Known>>,
}

#[derive(PartialEq, Eq, Hash)]
struct Asked {
    machine_id: String,
    folder: String,
}

struct Known {
    at: Instant,
    copy: Option<LocalCopy>,
}

/// A project's folder on the machine, and where sync keeps it on this Mac.
#[derive(Debug, Clone, PartialEq)]
struct LocalCopy {
    remote: String,
    local: PathBuf,
}

impl Copies {
    fn kept(&self, asked: &Asked) -> Option<Option<LocalCopy>> {
        let known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        let found = known.get(asked)?;
        (found.at.elapsed() < FRESH).then(|| found.copy.clone())
    }

    fn keep(&self, asked: Asked, copy: Option<LocalCopy>) {
        let mut known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        if known.len() >= MAX_KEPT {
            known.clear();
        }
        let at = Instant::now();
        known.insert(asked, Known { at, copy });
    }
}

/// `path` inside `root`, as the machine would name it.
pub(crate) fn joined(root: &str, path: &str) -> String {
    let root = root.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        root.to_owned()
    } else {
        format!("{root}/{path}")
    }
}

/// `path` relative to `folder`, `""` for the folder itself; `None` outside it.
fn under<'a>(path: &'a str, folder: &str) -> Option<&'a str> {
    let folder = folder.trim_end_matches('/');
    if folder.is_empty() {
        return None;
    }
    let rest = path.strip_prefix(folder)?;
    if rest.is_empty() {
        return Some(rest);
    }
    rest.strip_prefix('/')
}

/// The project whose folder on `machine_id` holds `dir`, the innermost when
/// they nest, and that folder.
fn holder<'a>(
    machine_id: &str,
    dir: &str,
    imported: &'a [ImportedProject],
    synced: &'a [ProjectSync],
) -> Option<(&'a str, &'a str)> {
    let imports = imported
        .iter()
        .filter(|row| row.machine_id == machine_id)
        .map(|row| (row.project.as_str(), row.folder.as_str()));
    let syncs = synced.iter().flat_map(|p| {
        p.machines
            .iter()
            .filter(|m| m.machine_id == machine_id)
            .map(|m| (p.project.as_str(), m.folder.as_str()))
    });
    imports
        .chain(syncs)
        .filter(|(_, folder)| under(dir, folder).is_some())
        .max_by_key(|(_, folder)| folder.len())
}

/// The folders on other machines that sync keeps in step with `remote` on
/// `machine_id`: Beep's sync of the project (`None` when paused), and the
/// Syncthing folder of the same label.
fn kept_with(
    machine_id: &str,
    remote: &str,
    synced: Option<&ProjectSync>,
    places: &[SyncthingPlace],
) -> Vec<String> {
    let beep = synced
        .filter(|p| {
            p.machines
                .iter()
                .any(|m| m.machine_id == machine_id && m.folder == remote)
        })
        .into_iter()
        .flat_map(|p| &p.machines)
        .filter(|m| m.machine_id != machine_id)
        .map(|m| m.folder.clone());
    let label = places
        .iter()
        .find(|p| p.machine_id == machine_id && p.folder == remote)
        .map(|p| p.label.as_str());
    let syncthing = places
        .iter()
        .filter(|p| p.machine_id != machine_id && Some(p.label.as_str()) == label)
        .map(|p| p.folder.clone());
    beep.chain(syncthing).collect()
}

/// Sync keeps `folder` on this Mac.
async fn is_here(folder: &Path, syncthing: &Syncthing) -> bool {
    let at = folder.to_owned();
    let found = spawn_blocking(move || (at.is_absolute() && at.is_dir(), keeps(&at))).await;
    match found {
        Ok((true, true)) => true,
        Ok((true, false)) => syncthing.check(folder).await.is_some(),
        Ok((false, _)) => false,
        Err(e) => {
            log::warn!("could not look for a synced copy: {e}");
            false
        }
    }
}

/// Asks the server how `dir` on `machine_id` is synced, and looks here.
async fn look(beep: &Beep, machine_id: &str, dir: &str) -> Result<Option<LocalCopy>> {
    let imported = beep.call(ListImportedProjects {}).await?;
    let synced = match beep.call(ListProjectSync {}).await {
        Err(e) if unknown_op(&e) => Vec::new(),
        other => other?,
    };
    let Some((project, remote)) = holder(machine_id, dir, &imported, &synced) else {
        return Ok(None);
    };
    let places = match beep
        .call(ListProjectSyncthing {
            project: project.to_owned(),
        })
        .await
    {
        Err(e) if unknown_op(&e) => Vec::new(),
        other => other?,
    };
    let sync = synced.iter().find(|p| p.project == project);
    let paused = match sync {
        Some(_) => match beep
            .call(GetProjectSyncDetail {
                project: project.to_owned(),
            })
            .await
        {
            Ok(detail) => detail.paused,
            Err(e) if unknown_op(&e) => false,
            Err(e) => return Err(e),
        },
        None => false,
    };
    let sync = sync.filter(|_| !paused);
    let syncthing = Syncthing::local();
    for folder in kept_with(machine_id, remote, sync, &places) {
        let local = PathBuf::from(folder);
        if is_here(&local, &syncthing).await {
            return Ok(Some(LocalCopy {
                remote: remote.to_owned(),
                local,
            }));
        }
    }
    Ok(None)
}

/// The copy here of `dir` on `machine_id`, when sync keeps one.
async fn copy_of(beep: &Beep, machine_id: &str, dir: &str) -> Option<LocalCopy> {
    // The Mac's own files are already here, through its own node.
    if beep.id().to_string() == machine_id {
        return None;
    }
    let asked = Asked {
        machine_id: machine_id.to_owned(),
        folder: dir.to_owned(),
    };
    if let Some(copy) = beep.copies().kept(&asked) {
        return copy;
    }
    match look(beep, machine_id, dir).await {
        Ok(copy) => {
            beep.copies().keep(asked, copy.clone());
            copy
        }
        Err(e) => {
            log::warn!("could not tell whether {dir} is synced here: {e:#}");
            None
        }
    }
}

/// The folder holding `path`, `""` when it has none.
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Part of the file `full` on `machine_id` from the copy here; `None` when
/// the node has to give it.
pub(crate) async fn read(
    beep: &Beep,
    machine_id: &str,
    full: &str,
    offset: u64,
    len: u32,
) -> Option<AgentFileChunk> {
    let copy = copy_of(beep, machine_id, parent(full)).await?;
    let rel = under(full, &copy.remote)?.to_owned();
    let found = spawn_blocking(move || local_read(&copy.local, &rel, offset, len)).await;
    match found {
        Ok(Ok(chunk)) => chunk,
        Ok(Err(e)) => {
            log::debug!("reading {full} from the node: {e:#}");
            None
        }
        Err(e) => {
            log::warn!("reading {full} from the node: {e}");
            None
        }
    }
}

/// The tree `ask` names from the copy here; `None` when the node has to
/// give it.
pub(crate) async fn tree(beep: &Beep, ask: &ListWorkTree) -> Option<WorkTree> {
    let root = &ask.root;
    let copy = copy_of(beep, &ask.machine_id, root).await?;
    let local = copy.local.join(under(root, &copy.remote)?);
    let (path, ignored, depth) = (ask.path.clone(), ask.ignored, ask.depth);
    let found = spawn_blocking(move || local_tree(&local, &path, ignored, depth)).await;
    match found {
        Ok(Ok(tree)) => Some(tree),
        Ok(Err(e)) => {
            log::debug!("listing {root} from the node: {e:#}");
            None
        }
        Err(e) => {
            log::warn!("listing {root} from the node: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::{MachineSync, SyncStatus};
    use std::time::UNIX_EPOCH;

    fn machine(id: &str, folder: &str) -> MachineSync {
        MachineSync {
            machine_id: id.into(),
            machine_name: id.into(),
            folder: folder.into(),
            status: SyncStatus::InSync {},
            files: 0,
            bytes: 0,
            pending: 0,
            conflicts: Vec::new(),
            rescan_secs: None,
            hint: None,
            error: None,
        }
    }

    fn sync(machines: Vec<MachineSync>) -> ProjectSync {
        ProjectSync {
            project: "acme/widget".into(),
            pinned: false,
            machines,
        }
    }

    fn place(id: &str, folder: &str, label: &str) -> SyncthingPlace {
        SyncthingPlace {
            machine_id: id.into(),
            folder: folder.into(),
            label: label.into(),
        }
    }

    fn import(id: &str, project: &str, folder: &str) -> ImportedProject {
        ImportedProject {
            project: project.into(),
            url: String::new(),
            machine_id: id.into(),
            machine_name: id.into(),
            folder: folder.into(),
            cloned: false,
            imported_at: UNIX_EPOCH,
        }
    }

    #[test]
    fn a_path_is_under_a_folder_only_at_a_name_boundary() {
        assert_eq!(under("/srv/app/a.glb", "/srv/app"), Some("a.glb"));
        assert_eq!(under("/srv/app", "/srv/app/"), Some(""));
        assert_eq!(under("/srv/apple/a.glb", "/srv/app"), None);
        assert_eq!(under("~/a.glb", "/srv/app"), None);
    }

    #[test]
    fn a_session_folder_belongs_to_the_innermost_project_on_its_machine() {
        let imported = [
            import("box", "acme/all", "/srv"),
            import("box", "acme/widget", "/srv/widget"),
            import("mac", "acme/other", "/srv/widget/sub"),
        ];
        assert_eq!(
            holder("box", "/srv/widget/sub", &imported, &[]),
            Some(("acme/widget", "/srv/widget"))
        );
        assert_eq!(holder("box", "/opt/else", &imported, &[]), None);
    }

    #[test]
    fn beep_sync_keeps_the_other_machines_folders_unless_paused() {
        let project = sync(vec![
            machine("box", "/srv/widget"),
            machine("mac", "/mac/widget"),
        ]);
        assert_eq!(
            kept_with("box", "/srv/widget", Some(&project), &[]),
            ["/mac/widget"]
        );
        // `look` passes no sync for a paused project.
        assert!(kept_with("box", "/srv/widget", None, &[]).is_empty());
        // The session's folder is not the one synced.
        assert!(kept_with("box", "/srv/other", Some(&project), &[]).is_empty());
    }

    #[test]
    fn syncthing_keeps_only_a_folder_of_the_same_label() {
        let places = [
            place("box", "/srv/widget", "widget"),
            place("mac", "/mac/widget", "widget"),
            place("air", "/air/widget", "something else"),
        ];
        assert_eq!(
            kept_with("box", "/srv/widget", None, &places),
            ["/mac/widget"]
        );
    }

    #[test]
    fn a_project_only_on_the_remote_machine_has_no_copy() {
        let places = [place("mac", "/mac/widget", "widget")];
        assert!(kept_with("box", "/srv/widget", None, &places).is_empty());
    }
}
