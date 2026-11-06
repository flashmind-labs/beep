//! Reads of a machine's project folder served from this Mac when the machine
//! is this Mac or the same project has a folder here. Sync, Syncthing's or
//! Beep's, is trusted to carry the files, so nothing is compared with the
//! machine and nothing asks whether sync is running. A project with no folder here, or a file not here yet, goes
//! to the node as if this were not here. The log says why a folder has no copy.

use crate::{
    beep::Beep,
    error::{DesktopError, Result},
    fleet::Fleet,
    projects::unknown_op,
};
use beep_mesh::{Key, Mesh};
use beep_node::agents::{local_read, local_tree};
use beep_proto::{
    AgentFileChunk, ImportedProject, ListImportedProjects, ListProjectSync, ListWorkTree,
    MachineSync, ProjectSync, SyncStatus, WorkTree,
    node::mesh_state::{StateValue, SyncSpec},
};
use iroh::EndpointId;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::{Duration, Instant, UNIX_EPOCH},
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

/// Why a folder on a machine has no copy here.
#[derive(Debug, PartialEq, thiserror::Error)]
enum Miss {
    #[error("no project holds it on that machine (removing a machine deletes its imports)")]
    NoProject,
    #[error("{project} has no folder on this Mac; its folders elsewhere: {tried:?}")]
    NotHere { project: String, tried: Vec<String> },
}

impl Copies {
    fn kept(&self, asked: &Asked) -> Option<Option<LocalCopy>> {
        let known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        let found = known.get(asked)?;
        (found.at.elapsed() < FRESH).then(|| found.copy.clone())
    }

    /// Whether no answer about the folder was standing: reads running at
    /// once all look, and their miss is logged once.
    fn keep(&self, asked: Asked, copy: Option<LocalCopy>) -> bool {
        let mut known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        if known.len() >= MAX_KEPT {
            known.clear();
        }
        let first = known
            .get(&asked)
            .is_none_or(|found| found.at.elapsed() >= FRESH);
        let at = Instant::now();
        known.insert(asked, Known { at, copy });
        first
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

/// The folders of `project` on machines other than `machine_id`.
fn other_folders<'a>(
    machine_id: &str,
    project: &str,
    imported: &'a [ImportedProject],
    synced: &'a [ProjectSync],
) -> Vec<&'a str> {
    let imports = imported
        .iter()
        .filter(|row| row.machine_id != machine_id && row.project.eq_ignore_ascii_case(project))
        .map(|row| row.folder.as_str());
    let syncs = synced
        .iter()
        .filter(|p| p.project.eq_ignore_ascii_case(project))
        .flat_map(|p| &p.machines)
        .filter(|m| m.machine_id != machine_id)
        .map(|m| m.folder.as_str());
    imports.chain(syncs).collect()
}

/// The project holding `dir` on `machine_id`, and the first of its other
/// folders that `is_here` says this Mac has.
fn find_copy(
    machine_id: &str,
    dir: &str,
    imported: &[ImportedProject],
    synced: &[ProjectSync],
    is_here: impl Fn(&Path) -> bool,
) -> Result<LocalCopy, Miss> {
    let Some((project, remote)) = holder(machine_id, dir, imported, synced) else {
        return Err(Miss::NoProject);
    };
    let tried = other_folders(machine_id, project, imported, synced);
    let here = tried
        .iter()
        .copied()
        .map(Path::new)
        .find(|folder| is_here(folder));
    let Some(local) = here else {
        return Err(Miss::NotHere {
            project: project.to_owned(),
            tried: tried.into_iter().map(str::to_owned).collect(),
        });
    };
    Ok(LocalCopy {
        remote: remote.to_owned(),
        local: local.to_owned(),
    })
}

/// `folder` is a folder here, and inside `root` once links resolve.
fn stays_inside(root: &Path, folder: &Path) -> bool {
    let (Ok(root), Ok(folder)) = (root.canonicalize(), folder.canonicalize()) else {
        return false;
    };
    folder.starts_with(root) && folder.is_dir()
}

/// What the mesh records of where projects are: their checkouts and the
/// folders Beep's sync keeps. Nothing is asked of any machine.
fn recorded(mesh: &Mesh) -> (Vec<ImportedProject>, Vec<ProjectSync>) {
    let names: HashMap<EndpointId, String> = mesh
        .devices()
        .into_iter()
        .map(|status| (status.device.id, status.device.name))
        .collect();
    let name_of = |id: &EndpointId| names.get(id).cloned().unwrap_or_default();
    let imported = beep_fleet::project_names(mesh)
        .into_iter()
        .flat_map(|project| {
            beep_fleet::places(mesh, &project)
                .into_iter()
                .map(move |(machine, folder)| (project.clone(), machine, folder))
        })
        .map(|(project, machine, folder)| ImportedProject {
            project,
            url: String::new(),
            machine_id: machine.to_string(),
            machine_name: name_of(&machine),
            folder,
            cloned: false,
            imported_at: UNIX_EPOCH,
        })
        .collect();
    let synced = mesh
        .scan("sync/")
        .into_iter()
        .filter_map(|(key, value)| {
            let Key::Sync(project) = key else {
                return None;
            };
            let spec = SyncSpec::from_bytes(&value).ok()?;
            let machines = spec
                .folders
                .into_iter()
                .map(|(machine, folder)| MachineSync {
                    machine_id: machine.to_string(),
                    machine_name: name_of(&machine),
                    folder,
                    status: SyncStatus::InSync {},
                    files: 0,
                    bytes: 0,
                    pending: 0,
                    conflicts: Vec::new(),
                    rescan_secs: None,
                    hint: None,
                    error: None,
                })
                .collect();
            Some(ProjectSync {
                project,
                pinned: false,
                machines,
            })
        })
        .collect();
    (imported, synced)
}

/// Looks for this Mac's copy of `dir` among the projects the mesh records,
/// then, with a server linked and no mesh project holding it, among the
/// server's. The outer error is a failed request, which says nothing about
/// the copy.
async fn look(
    beep: &Beep,
    fleet: &Fleet,
    machine_id: &str,
    dir: &str,
) -> Result<Result<LocalCopy, Miss>> {
    let (mut imported, mut synced) = fleet.mesh().map(|mesh| recorded(&mesh)).unwrap_or_default();
    let here = |folder: &Path| folder.is_absolute() && folder.is_dir();
    let (machine, folder) = (machine_id.to_owned(), dir.to_owned());
    if holder(&machine, &folder, &imported, &synced).is_none() && beep.has_server() {
        imported = beep.call(ListImportedProjects {}).await?;
        synced = match beep.call(ListProjectSync {}).await {
            Err(e) if unknown_op(&e) => Vec::new(),
            other => other?,
        };
    }
    spawn_blocking(move || find_copy(&machine, &folder, &imported, &synced, here))
        .await
        .map_err(|e| DesktopError::Other(e.into()))
}

/// The copy here of `dir` on `machine_id`, when this Mac has the project.
async fn copy_of(beep: &Beep, fleet: &Fleet, machine_id: &str, dir: &str) -> Option<LocalCopy> {
    // The Mac's own folder is the copy: no trip through its node.
    if fleet.id().to_string() == machine_id {
        return Some(LocalCopy {
            remote: dir.to_owned(),
            local: PathBuf::from(dir),
        });
    }
    let asked = Asked {
        machine_id: machine_id.to_owned(),
        folder: dir.to_owned(),
    };
    if let Some(copy) = beep.copies().kept(&asked) {
        return copy;
    }
    match look(beep, fleet, machine_id, dir).await {
        Ok(Ok(copy)) => {
            beep.copies().keep(asked, Some(copy.clone()));
            Some(copy)
        }
        Ok(Err(miss)) => {
            if beep.copies().keep(asked, None) {
                log::info!("{dir} on {machine_id} reads from the node: {miss}");
            }
            None
        }
        // Not kept: the next read asks again.
        Err(e) => {
            log::warn!("could not tell whether {dir} is synced here: {e:#}");
            None
        }
    }
}

/// Where to open `rel` of the copy at `local`: its top when `rel` is not
/// here, as a linked worktree stays on its machine.
fn place_or_top(local: PathBuf, rel: &str) -> PathBuf {
    let place = local.join(rel);
    if stays_inside(&local, &place) {
        place
    } else {
        local
    }
}

/// The folder on this Mac for `dir` on `machine_id`, when this Mac has it.
pub(crate) async fn folder_here(
    beep: &Beep,
    fleet: &Fleet,
    machine_id: &str,
    dir: &str,
) -> Option<PathBuf> {
    let copy = copy_of(beep, fleet, machine_id, dir).await?;
    let rel = under(dir, &copy.remote)?.to_owned();
    spawn_blocking(move || place_or_top(copy.local, &rel))
        .await
        .ok()
}

/// The folder on this Mac that is exactly `dir` on `machine_id`: `None` when
/// this Mac has no copy of the project, or the copy lacks `dir` (unlike
/// `folder_here`, which falls back to the copy's top).
pub(crate) async fn exact_folder_here(
    beep: &Beep,
    fleet: &Fleet,
    machine_id: &str,
    dir: &str,
) -> Option<PathBuf> {
    let copy = copy_of(beep, fleet, machine_id, dir).await?;
    let rel = under(dir, &copy.remote)?.to_owned();
    spawn_blocking(move || {
        let place = copy.local.join(rel);
        stays_inside(&copy.local, &place).then_some(place)
    })
    .await
    .ok()
    .flatten()
}

/// The folder holding `path`, `""` when it has none.
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Part of the file `full` on `machine_id` from the copy here; `None` when
/// the node has to give it.
pub(crate) async fn read(
    beep: &Beep,
    fleet: &Fleet,
    machine_id: &str,
    full: &str,
    offset: u64,
    len: u32,
) -> Option<AgentFileChunk> {
    let copy = copy_of(beep, fleet, machine_id, parent(full)).await?;
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
pub(crate) async fn tree(beep: &Beep, fleet: &Fleet, ask: &ListWorkTree) -> Option<WorkTree> {
    let root = &ask.root;
    let copy = copy_of(beep, fleet, &ask.machine_id, root).await?;
    let local = copy.local.join(under(root, &copy.remote)?);
    let (path, ignored, depth) = (ask.path.clone(), ask.ignored, ask.depth);
    let found = spawn_blocking(move || {
        if !stays_inside(&copy.local, &local) {
            return Ok(None);
        }
        local_tree(&local, &path, ignored, depth).map(Some)
    })
    .await;
    match found {
        Ok(Ok(tree)) => tree,
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
    use std::{env, fs, os::unix::fs::symlink, process};

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

    /// What `find_copy` finds when this Mac has only the folders `here`.
    fn found(
        dir: &str,
        imported: &[ImportedProject],
        synced: &[ProjectSync],
        here: &[&str],
    ) -> Result<LocalCopy, Miss> {
        find_copy("box", dir, imported, synced, |folder| {
            here.iter().any(|there| folder == Path::new(there))
        })
    }

    fn copy(remote: &str, local: &str) -> LocalCopy {
        LocalCopy {
            remote: remote.into(),
            local: local.into(),
        }
    }

    #[test]
    fn a_project_imported_on_both_reads_here_whatever_the_case_or_slash() {
        let imported = [
            import("box", "Acme/Widget", "/srv/widget/"),
            import("mac", "acme/widget", "/mac/code/widget"),
        ];
        assert_eq!(
            found("/srv/widget", &imported, &[], &["/mac/code/widget"]),
            Ok(copy("/srv/widget/", "/mac/code/widget"))
        );
    }

    #[test]
    fn a_project_synced_by_beep_only_reads_here() {
        let synced = [sync(vec![
            machine("box", "/srv/widget"),
            machine("mac", "/mac/widget"),
        ])];
        assert_eq!(
            found("/srv/widget", &[], &synced, &["/mac/widget"]),
            Ok(copy("/srv/widget", "/mac/widget"))
        );
    }

    #[test]
    fn another_project_of_the_same_folder_name_is_not_the_copy() {
        let imported = [
            import("box", "acme/widget", "/srv/widget"),
            import("mac", "other/widget", "/mac/widget"),
        ];
        assert_eq!(
            found("/srv/widget", &imported, &[], &["/mac/widget"]),
            Err(Miss::NotHere {
                project: "acme/widget".into(),
                tried: Vec::new(),
            })
        );
    }

    #[test]
    fn a_session_in_a_subfolder_or_worktree_reads_under_the_projects_folder() {
        let imported = [
            import("box", "acme/widget", "/srv/widget"),
            import("mac", "acme/widget", "/mac/widget"),
        ];
        for dir in ["/srv/widget/assets", "/srv/widget/.worktrees/fix/assets"] {
            assert_eq!(
                found(dir, &imported, &[], &["/mac/widget"]),
                Ok(copy("/srv/widget", "/mac/widget")),
                "{dir}"
            );
        }
        assert_eq!(
            under("/srv/widget/.worktrees/fix/a.glb", "/srv/widget"),
            Some(".worktrees/fix/a.glb")
        );
    }

    #[test]
    fn a_folder_not_here_is_a_miss_that_names_where_it_looked() {
        let imported = [
            import("box", "acme/widget", "/srv/widget"),
            import("mac", "acme/widget", "/mac/widget"),
        ];
        assert_eq!(
            found("/srv/widget", &imported, &[], &[]),
            Err(Miss::NotHere {
                project: "acme/widget".into(),
                tried: vec!["/mac/widget".into()],
            })
        );
    }

    #[test]
    fn a_machine_that_lost_its_imports_holds_no_project() {
        let imported = [import("mac", "acme/widget", "/mac/widget")];
        assert_eq!(
            found("/srv/widget", &imported, &[], &["/mac/widget"]),
            Err(Miss::NoProject)
        );
    }

    #[test]
    fn nothing_outside_the_local_folder_counts_as_inside_it() {
        let dir = env::temp_dir().join(format!("beep-work-copy-{}", process::id()));
        let (root, outside) = (dir.join("widget"), dir.join("outside"));
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, root.join("link")).unwrap();
        assert!(stays_inside(&root, &root));
        assert!(stays_inside(&root, &root.join("assets")));
        assert!(!stays_inside(&root, &root.join("link")));
        assert!(!stays_inside(&root, &root.join("missing")));
        assert!(!stays_inside(&root, &root.join("../outside")));
        // What a doubled slash in a session's path would join to.
        assert!(!stays_inside(&root, &root.join(&outside)));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_folder_that_did_not_sync_opens_the_top_of_the_copy() {
        let dir = env::temp_dir().join(format!("beep-work-copy-top-{}", process::id()));
        let local = dir.join("widget");
        fs::create_dir_all(local.join("sdk")).unwrap();
        assert_eq!(place_or_top(local.clone(), "sdk"), local.join("sdk"));
        assert_eq!(place_or_top(local.clone(), ".worktrees/fix"), local);
        assert_eq!(place_or_top(local.clone(), "../other"), local);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_miss_counts_as_first_once_per_window() {
        let copies = Copies::default();
        let asked = || Asked {
            machine_id: "box".into(),
            folder: "/srv/widget".into(),
        };
        assert!(copies.keep(asked(), None));
        assert!(!copies.keep(asked(), None));
        let long_ago = Instant::now().checked_sub(FRESH).unwrap();
        copies.known.lock().unwrap().get_mut(&asked()).unwrap().at = long_ago;
        assert!(copies.keep(asked(), None));
    }
}
