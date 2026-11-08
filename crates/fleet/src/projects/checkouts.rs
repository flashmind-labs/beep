//! Adding, relocating and removing a project's checkouts on mesh machines.

use super::store::row;
use crate::{
    desk::{Desk, Seen},
    error::{FleetError, Result},
    records::{
        Place, ProjectRecord, WRITES, find_project, now_ms, place_key, project_key, projects, read,
        sync_checkouts, write,
    },
};
use beep_mesh::Key;
use beep_projects::{
    imports::{
        ImportError, flow,
        folder::{self, FolderPath},
        hosts::forgejo_of,
        repo::same_repo,
    },
    is_windows,
    model::{plain_path, windows_path},
};
use beep_proto::{ImportedProject, MachineFolder, ProjectBranches, node::mesh_state::SyncSpec};
use iroh::EndpointId;

pub(super) const FALLBACK_DEFAULT: &str = "main";
const LAST_MACHINE: &str = "A project stays on at least one machine.";

/// What `folder` holds on the online machine `seen`, with its path as the node there spells it:
/// the sh of Git for Windows says `/c/Users/me`, and the node reads `C:\Users\me`.
async fn look_at(seen: &Seen, folder: &str) -> Result<(FolderPath, MachineFolder)> {
    let name = &seen.machine.name;
    if !seen.machine.online {
        return Err(ImportError::Offline(name.clone()).into());
    }
    let asked = FolderPath::parse(folder)?.ok_or(ImportError::BadFolder)?;
    let mut there = folder::look(seen.machine.shell()?, name, &asked).await?;
    let windows = seen.machine.os.as_deref().is_some_and(is_windows);
    if let Some(native) = windows.then(|| windows_path(&there.path)).flatten() {
        there.path = native;
    }
    Ok((asked, there))
}

/// The folder's path, refused when Syncthing keeps it in step there: Beep's sync would hold it,
/// so nothing would ever arrive.
async fn refuse_syncthing(desk: &Desk, seen: &Seen, there: MachineFolder) -> Result<String> {
    if seen
        .env
        .as_ref()
        .is_some_and(|env| env.syncthing_folder(&there.path).is_some())
    {
        if there.exists
            && desk
                .syncthing_files(&seen.machine.id, &there.path, None)
                .await
                .is_ok_and(|files| files.available && !files.enabled)
        {
            return Ok(there.path);
        }
        return Err(FleetError::refused(format!(
            "Syncthing keeps {} in step on {}. Choose another folder.",
            there.path, seen.machine.name
        )));
    }
    Ok(there.path)
}

impl Desk {
    /// Reads branches from an online checkout, nearest first.
    pub async fn branches(&self, project: &str) -> Result<ProjectBranches> {
        let mut places = crate::places(&self.mesh, project);
        crate::nearest_first(&self.mesh, &mut places, |(machine, _)| *machine);
        for (id, folder) in places {
            let Some(seen) = self.seen_one(id).await else {
                continue;
            };
            if !seen.machine.online {
                continue;
            }
            let (default_branch, branches) = beep_projects::branches::read_branches(
                seen.machine.shell()?,
                &folder,
                FALLBACK_DEFAULT,
            )
            .await?;
            return Ok(ProjectBranches {
                machine_id: id.to_string(),
                machine_name: seen.machine.name,
                default_branch,
                branches,
            });
        }
        Err(FleetError::refused(
            "No machine with a checkout of the project is online.",
        ))
    }

    /// Puts `project` on `machine` in `folder`, which is missing or empty: makes the folder and
    /// records the checkout, and with it the project's sync, which brings the files and `.git`.
    pub async fn add_to_machine(
        &self,
        project: &str,
        machine: &str,
        folder: &str,
    ) -> Result<ImportedProject> {
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        let id: EndpointId = machine.parse().map_err(|_| ImportError::NoSuchMachine)?;
        let seen = self.seen_one(id).await.ok_or(ImportError::NoSuchMachine)?;
        let name = &seen.machine.name;
        if let Some(place) = stored.places.get(&id) {
            return Err(FleetError::refused(format!(
                "{} is already on {name}, in {}.",
                stored.name, place.folder
            )));
        }
        // A record with no machines, or one this build cannot read, stays as it is.
        let sync = Key::Sync(stored.name.clone());
        if self.mesh.get(&sync).is_some()
            && read::<SyncSpec>(&self.mesh, &sync).is_none_or(|spec| spec.folders.is_empty())
        {
            return Err(FleetError::refused(format!(
                "{} is pinned to where it is: it never syncs.",
                stored.name
            )));
        }
        let (asked, there) = look_at(&seen, folder).await?;
        if there.exists && !there.empty {
            return Err(FleetError::refused(format!(
                "{} has other files. Choose another folder.",
                there.path
            )));
        }
        let path = refuse_syncthing(self, &seen, there).await?;
        folder::make(seen.machine.shell()?, name, &asked).await?;

        let _write = WRITES.lock().await;
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        let place = Place {
            folder: path,
            imported_ms: now_ms(),
            // `.git/config` comes from the others, and with it their `origin`.
            fork: stored.places.values().find_map(|place| place.fork.clone()),
        };
        write(&self.mesh, place_key(&stored.name, id), Some(&place)).await?;
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        sync_checkouts(&self.mesh, &stored).await?;
        Ok(flow::project(row(&stored, id, &place), name.clone(), false))
    }

    /// Points `project` on `machine` at `folder`, leaving the folder it had on disk as it is. The
    /// new folder is missing or empty, and the others fill it as they fill one Add made, or holds
    /// a checkout of the same repository, which joins the sync and merges with the others as two
    /// checkouts do. Nothing is moved: moving a checkout under a running sync is how files get
    /// lost.
    pub async fn change_folder(
        &self,
        project: &str,
        machine: &str,
        folder: &str,
    ) -> Result<ImportedProject> {
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        let id: EndpointId = machine.parse().map_err(|_| ImportError::NoSuchMachine)?;
        let seen = self.seen_one(id).await.ok_or(ImportError::NoSuchMachine)?;
        let name = &seen.machine.name;
        let Some(old) = stored.places.get(&id).cloned() else {
            return Err(FleetError::refused(format!(
                "{} is not on {name}.",
                stored.name
            )));
        };
        let (asked, there) = look_at(&seen, folder).await?;
        if plain_path(&there.path) == plain_path(&old.folder) {
            return Err(FleetError::refused(format!(
                "{} is already in {} on {name}.",
                stored.name,
                plain_path(&old.folder)
            )));
        }
        let checkout = there.origin.as_deref().is_some_and(|origin| {
            same_repo(&stored.record.url, origin, &forgejo_of(&self.opened()))
        });
        if there.exists && !there.empty && !checkout {
            return Err(FleetError::refused(format!(
                "{} has other files. Choose an empty folder or a checkout of {}.",
                there.path, stored.name
            )));
        }
        // An empty folder is filled by the machines it syncs with; with none, nothing would come.
        let sync = Key::Sync(stored.name.clone());
        let fills = read::<SyncSpec>(&self.mesh, &sync)
            .is_some_and(|spec| spec.folders.keys().any(|other| *other != id));
        if !checkout && !fills {
            return Err(FleetError::refused(format!(
                "No other machine syncs {} to fill it. Choose a checkout of it.",
                stored.name
            )));
        }
        let exists = there.exists;
        let path = refuse_syncthing(self, &seen, there).await?;
        if !exists {
            folder::make(seen.machine.shell()?, name, &asked).await?;
        }

        let _write = WRITES.lock().await;
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        let place = Place {
            folder: path,
            ..stored.places.get(&id).cloned().unwrap_or(old)
        };
        write(&self.mesh, place_key(&stored.name, id), Some(&place)).await?;
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        sync_checkouts(&self.mesh, &stored).await?;
        Ok(flow::project(row(&stored, id, &place), name.clone(), false))
    }

    /// Takes `project` off `machine`: the machine leaves the project's sync and its checkout record
    /// goes, so the node there stops syncing the folder and leaves every file in it as it is. With
    /// one machine left the sync record goes too. Only an import or Add writes a checkout record,
    /// so the machine comes back only when the user puts the project there again. The last
    /// machine is refused: a project stays on at least one.
    ///
    /// With `delete_folder`, the folder there is deleted too, once its node has stopped syncing
    /// it, so the delete never reaches the others. Says what became of it then.
    pub async fn remove_from_machine(
        &self,
        project: &str,
        machine: &str,
        delete_folder: bool,
    ) -> Result<Option<String>> {
        let id: EndpointId = machine.parse().map_err(|_| ImportError::NoSuchMachine)?;
        let (name, left) = self.leave(project, id).await?;
        if !delete_folder {
            return Ok(None);
        }
        Ok(Some(self.delete_left_folder(&name, id, &left).await))
    }

    /// Takes `project` off `machine` in the records; gives its name and the folder it had there.
    async fn leave(&self, project: &str, id: EndpointId) -> Result<(String, String)> {
        let _write = WRITES.lock().await;
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        let Some(place) = stored.places.get(&id) else {
            return Err(FleetError::refused(format!(
                "{} is not on {}.",
                stored.name,
                self.name_of(id)
            )));
        };
        let folder = place.folder.clone();
        // A machine gone from the mesh holds nothing, so only members count.
        let members = stored
            .places
            .keys()
            .filter(|on| self.mesh.is_member(**on))
            .count();
        if self.mesh.is_member(id) && members < 2 {
            return Err(FleetError::refused(LAST_MACHINE));
        }
        let key = Key::Sync(stored.name.clone());
        if self.mesh.get(&key).is_some() {
            let Some(mut spec) = read::<SyncSpec>(&self.mesh, &key) else {
                return Err(FleetError::refused(
                    "A newer Beep wrote this project's sync. Update Beep here first.",
                ));
            };
            // A pinned project names no machine: it stays pinned.
            if spec.folders.remove(&id).is_some() {
                let left = (spec.folders.len() >= 2).then_some(&spec);
                write(&self.mesh, key, left).await?;
            }
        }
        write::<Place>(&self.mesh, place_key(&stored.name, id), None).await?;
        Ok((stored.name, folder))
    }

    /// Puts every project with checkouts on two machines or more in sync, for those imported
    /// before that was the rule.
    pub(crate) async fn sync_all_checkouts(&self) -> Result<()> {
        let _write = WRITES.lock().await;
        for stored in projects(&self.mesh) {
            sync_checkouts(&self.mesh, &stored).await?;
        }
        Ok(())
    }

    /// Forgets a project: its checkouts stay on the machines, and so do their files.
    pub async fn remove(&self, name: &str) -> Result<()> {
        let _write = WRITES.lock().await;
        let name = name.trim();
        let stored = find_project(&self.mesh, name).ok_or(FleetError::NotFound)?;
        for machine in stored.places.keys() {
            self.mesh
                .set(place_key(&stored.name, *machine), None)
                .await?;
        }
        write::<ProjectRecord>(&self.mesh, project_key(&stored.name), None).await?;
        for key in [Key::Sync(stored.name.clone()), Key::Pin(stored.name)] {
            if self.mesh.get(&key).is_some() {
                self.mesh.set(key, None).await?;
            }
        }
        Ok(())
    }

    /// Whether the mesh has the project `name`.
    pub fn has_project(&self, name: &str) -> bool {
        find_project(&self.mesh, name).is_some()
    }
}
