//! Adding, relocating and removing a project's checkouts on mesh machines.

use super::store::row;
use crate::{
    desk::{Desk, Seen},
    error::{FleetError, Result},
    records::{
        Place, ProjectRecord, Stored, WRITES, find_project, now_ms, place_key, project_key,
        projects, read, sync_checkouts, write,
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
use beep_proto::{
    ImportedProject, MachineFolder, ProjectBranches,
    node::mesh_state::{ProjectSyncSettings, SyncSpec},
};
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

/// The folder's path, made when missing and out of the Syncthing share that holds it there:
/// Beep's sync would wait on Syncthing, so nothing would ever arrive. The node there reads the
/// folder before its ignore rules gain it, so it is made first. A project set to use Syncthing
/// is refused instead, before anything is made.
async fn make_and_leave_syncthing(
    desk: &Desk,
    seen: &Seen,
    asked: &FolderPath,
    there: MachineFolder,
    record: &ProjectRecord,
) -> Result<String> {
    let shared = seen
        .env
        .as_ref()
        .is_some_and(|env| env.syncthing_folder(&there.path).is_some());
    let sync = read::<ProjectSyncSettings>(&desk.mesh, &Key::ProjectSync).unwrap_or_default();
    let leave = (record.syncthing_choice(&sync) != Some(true)).then_some(false);
    let refused = || {
        FleetError::refused(format!(
            "Syncthing keeps {} in step on {}, and this project uses Syncthing. Choose another folder.",
            there.path, seen.machine.name
        ))
    };

    if shared && leave.is_none() && !there.exists {
        return Err(refused());
    }

    if !there.exists {
        folder::make(seen.machine.shell()?, &seen.machine.name, asked).await?;
    }

    if !shared {
        return Ok(there.path);
    }

    let files = desk
        .syncthing_files(&seen.machine.id, &there.path, leave)
        .await?;

    if files.available && files.enabled {
        return Err(refused());
    }

    Ok(there.path)
}

/// Refuses a folder the project cannot take. It takes one that is missing or empty, which the
/// others fill, a checkout of the same repository, which joins as it is, and for a repository
/// one with files and no `.git`: the history arrives and the files stay as they are, local
/// changes where they differ. A folder without Git merges every file into every machine, so
/// its folder starts empty. Says whether the others have to fill it.
fn takes(desk: &Desk, stored: &Stored, there: &MachineFolder) -> Result<bool> {
    let Some(origin) = there.origin.as_deref() else {
        if there.exists && !there.empty && stored.record.url.is_empty() {
            return Err(FleetError::refused(format!(
                "{} has other files. Choose an empty folder.",
                there.path
            )));
        }

        return Ok(true);
    };

    if same_repo(&stored.record.url, origin, &forgejo_of(&desk.opened())) {
        return Ok(false);
    }

    Err(FleetError::refused(format!(
        "{} is a checkout of another repository. Choose an empty folder or a checkout of {}.",
        there.path, stored.name
    )))
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

    /// Puts `project` on `machine` in `folder`, one the project [`takes`]: makes the folder and
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
        takes(self, &stored, &there)?;
        let path = make_and_leave_syncthing(self, &seen, &asked, there, &stored.record).await?;

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

    /// Points `project` on `machine` at `folder`, one the project [`takes`], leaving the folder it
    /// had on disk as it is. Nothing is moved: moving a checkout under a running sync is how
    /// files get lost.
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

        let filled = takes(self, &stored, &there)?;
        // The machines it syncs with fill it; with none, nothing would come.
        let sync = Key::Sync(stored.name.clone());
        let fills = read::<SyncSpec>(&self.mesh, &sync)
            .is_some_and(|spec| spec.folders.keys().any(|other| *other != id));
        if filled && !fills {
            return Err(FleetError::refused(format!(
                "No other machine syncs {} to fill it. Choose a checkout of it.",
                stored.name
            )));
        }

        let path = make_and_leave_syncthing(self, &seen, &asked, there, &stored.record).await?;

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
                self.mesh.name_of(id)
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
}
