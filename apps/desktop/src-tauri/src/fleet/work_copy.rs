//! Reads of a machine's project folder served from this Mac when the machine
//! is this Mac or the same project has a folder here. Sync, Syncthing's or
//! Beep's, is trusted to carry the files, so nothing is compared with the
//! machine and nothing asks whether sync is running. A project with no folder here, or a file not here yet, goes
//! to the node as if this were not here. The log says why a folder has no copy.

use super::{
    Fleet,
    copies::{Asked, LocalCopy},
};
use crate::error::{DesktopError, Result};
use beep_mesh::{Key, Mesh};
#[cfg(desktop)]
use beep_node::agents::{local_hashes, local_read, local_tree};
use beep_proto::{
    AgentFileChunk, ListWorkTree, WorkTree,
    node::mesh_state::{StateValue, SyncSpec},
};
use std::path::{Path, PathBuf};
use tokio::task::spawn_blocking;

/// A phone reads files from the remote machine.
#[cfg(mobile)]
fn local_read(
    _root: &Path,
    _path: &str,
    _offset: u64,
    _len: u32,
) -> anyhow::Result<Option<AgentFileChunk>> {
    Ok(None)
}

/// A phone asks the remote machine for file hashes.
#[cfg(mobile)]
fn local_hashes(_root: &Path, _paths: &[String]) -> anyhow::Result<Vec<Option<[u8; 32]>>> {
    anyhow::bail!("this build reads project files from a remote machine")
}

/// A phone asks the remote machine for its tree.
#[cfg(mobile)]
fn local_tree(_root: &Path, _path: &str, _ignored: bool, _depth: u32) -> anyhow::Result<WorkTree> {
    anyhow::bail!("this build reads project files from a remote machine")
}

/// Why a folder on a machine has no copy here.
#[derive(Debug, PartialEq, thiserror::Error)]
enum Miss {
    #[error("no project holds it on that machine (removing a machine deletes its imports)")]
    NoProject,
    #[error("{project} has no folder on this computer; its folders elsewhere: {tried:?}")]
    NotHere { project: String, tried: Vec<String> },
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

/// A project's folder on one machine, from a checkout or a Beep sync.
struct Place {
    project: String,
    machine_id: String,
    folder: String,
}

/// The project whose folder on `machine_id` holds `dir`, the innermost when
/// they nest, and that folder.
fn holder<'a>(machine_id: &str, dir: &str, places: &'a [Place]) -> Option<(&'a str, &'a str)> {
    places
        .iter()
        .filter(|place| place.machine_id == machine_id)
        .map(|place| (place.project.as_str(), place.folder.as_str()))
        .filter(|(_, folder)| under(dir, folder).is_some())
        .max_by_key(|(_, folder)| folder.len())
}

/// The folders of `project` on machines other than `machine_id`.
fn other_folders<'a>(machine_id: &str, project: &str, places: &'a [Place]) -> Vec<&'a str> {
    places
        .iter()
        .filter(|place| {
            place.machine_id != machine_id && place.project.eq_ignore_ascii_case(project)
        })
        .map(|place| place.folder.as_str())
        .collect()
}

/// The project holding `dir` on `machine_id`, and the first of its other
/// folders that `is_here` says this Mac has.
fn find_copy(
    machine_id: &str,
    dir: &str,
    places: &[Place],
    is_here: impl Fn(&Path) -> bool,
) -> Result<LocalCopy, Miss> {
    let Some((project, remote)) = holder(machine_id, dir, places) else {
        return Err(Miss::NoProject);
    };
    let tried = other_folders(machine_id, project, places);
    let here = tried
        .iter()
        .copied()
        .map(Path::new)
        // Git working trees can differ even at the same commit.
        .find(|folder| is_here(folder) && !folder.join(".git").exists());
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
fn recorded(mesh: &Mesh) -> Vec<Place> {
    let imported = beep_fleet::project_names(mesh)
        .into_iter()
        .flat_map(|project| {
            beep_fleet::places(mesh, &project)
                .into_iter()
                .map(move |(machine, folder)| Place {
                    project: project.clone(),
                    machine_id: machine.to_string(),
                    folder,
                })
        });
    let synced = mesh
        .scan("sync/")
        .into_iter()
        .filter_map(|(key, value)| {
            let Key::Sync(project) = key else {
                return None;
            };
            let spec = SyncSpec::from_bytes(&value).ok()?;
            Some((project, spec.folders))
        })
        .flat_map(|(project, folders)| {
            folders.into_iter().map(move |(machine, folder)| Place {
                project: project.clone(),
                machine_id: machine.to_string(),
                folder,
            })
        });
    imported.chain(synced).collect()
}

/// Looks for this Mac's copy of `dir` among the projects the mesh records.
/// The outer error is a failed task, which says nothing about the copy.
async fn look(fleet: &Fleet, machine_id: &str, dir: &str) -> Result<Result<LocalCopy, Miss>> {
    let places = fleet.mesh().map(|mesh| recorded(&mesh)).unwrap_or_default();
    let here = |folder: &Path| folder.is_absolute() && folder.is_dir();
    let (machine, folder) = (machine_id.to_owned(), dir.to_owned());

    spawn_blocking(move || find_copy(&machine, &folder, &places, here))
        .await
        .map_err(|e| DesktopError::Other(e.into()))
}

/// The copy here of `dir` on `machine_id`, when this Mac has the project.
async fn copy_of(fleet: &Fleet, machine_id: &str, dir: &str) -> Option<LocalCopy> {
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

    if let Some(copy) = fleet.copies().kept(&asked) {
        let copy = copy?;

        if tokio::fs::try_exists(copy.local.join(".git"))
            .await
            .unwrap_or(true)
        {
            return None;
        }

        return Some(copy);
    }

    match look(fleet, machine_id, dir).await {
        Ok(Ok(copy)) => {
            fleet.copies().keep(asked, Some(copy.clone()));
            Some(copy)
        }
        Ok(Err(miss)) => {
            if fleet.copies().keep(asked, None) {
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
pub(crate) async fn folder_here(fleet: &Fleet, machine_id: &str, dir: &str) -> Option<PathBuf> {
    let copy = copy_of(fleet, machine_id, dir).await?;
    let rel = under(dir, &copy.remote)?.to_owned();
    spawn_blocking(move || place_or_top(copy.local, &rel))
        .await
        .ok()
}

/// The folder on this Mac that is exactly `dir` on `machine_id`: `None` when
/// this Mac has no copy of the project, or the copy lacks `dir` (unlike
/// `folder_here`, which falls back to the copy's top).
pub(crate) async fn exact_folder_here(
    fleet: &Fleet,
    machine_id: &str,
    dir: &str,
) -> Option<PathBuf> {
    let copy = copy_of(fleet, machine_id, dir).await?;
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
    fleet: &Fleet,
    machine_id: &str,
    full: &str,
    offset: u64,
    len: u32,
) -> Option<AgentFileChunk> {
    let copy = copy_of(fleet, machine_id, parent(full)).await?;
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
pub(crate) async fn tree(fleet: &Fleet, ask: &ListWorkTree) -> Option<WorkTree> {
    let root = &ask.root;
    let copy = copy_of(fleet, &ask.machine_id, root).await?;
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

/// The hashes of `paths` in `root` from the copy here, which reads come
/// from too; `None` when the node has to give them.
pub(crate) async fn hashes(
    fleet: &Fleet,
    machine_id: &str,
    root: &str,
    paths: Vec<String>,
) -> Option<Vec<Option<[u8; 32]>>> {
    let copy = copy_of(fleet, machine_id, root).await?;
    let local = copy.local.join(under(root, &copy.remote)?);
    let found = spawn_blocking(move || {
        if !stays_inside(&copy.local, &local) {
            return Ok(None);
        }

        local_hashes(&local, &paths).map(Some)
    })
    .await;

    match found {
        Ok(Ok(hashes)) => hashes,
        Ok(Err(e)) => {
            log::debug!("hashing in {root} from the node: {e:#}");
            None
        }
        Err(e) => {
            log::warn!("hashing in {root} from the node: {e}");
            None
        }
    }
}

#[cfg(test)]
#[path = "../tests/fleet_work_copy.rs"]
mod tests;
