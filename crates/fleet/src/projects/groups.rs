//! Project groups and pins in the mesh.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    records::{GroupRecord, PinRecord, WRITES, now_ms, read, scan, write},
};
use beep_mesh::Key;
use beep_proto::{
    ProjectGroup,
    groups::{group_name, group_paths},
};
use uuid::Uuid;

impl Desk {
    /// The projects pinned to the top of the sidebar, oldest pin first.
    pub fn on_top(&self) -> Vec<String> {
        let mut pins: Vec<(u64, String)> = scan::<PinRecord>(&self.mesh, "pin/")
            .into_iter()
            .filter_map(|(key, pin)| match key {
                Key::Pin(name) => Some((pin.at_ms, name)),
                _ => None,
            })
            .collect();
        pins.sort();
        pins.into_iter().map(|(_, name)| name).collect()
    }

    /// Pins `project` to the top of the sidebar, or unpins it.
    pub async fn pin(&self, project: &str, pinned: bool) -> Result<()> {
        let _write = WRITES.lock().await;
        let key = Key::Pin(project.to_owned());
        if pinned {
            if read::<PinRecord>(&self.mesh, &key).is_none() {
                write(&self.mesh, key, Some(&PinRecord { at_ms: now_ms() })).await?;
            }
        } else if self.mesh.get(&key).is_some() {
            self.mesh.set(key, None).await?;
        }

        Ok(())
    }

    /// The user's groups, oldest first.
    pub fn groups(&self) -> Vec<ProjectGroup> {
        let mut groups: Vec<(u64, ProjectGroup)> = scan::<GroupRecord>(&self.mesh, "group/")
            .into_iter()
            .filter_map(|(key, record)| {
                let Key::Group(id) = key else { return None };
                Some((
                    record.created_ms,
                    ProjectGroup {
                        id: id.parse().ok()?,
                        name: record.name,
                        paths: record.paths,
                    },
                ))
            })
            .collect();
        groups.sort_by_key(|(created, _)| *created);
        groups.into_iter().map(|(_, group)| group).collect()
    }

    /// Makes a group without `id`, else changes the group `id`.
    pub async fn save_group(
        &self,
        id: Option<Uuid>,
        name: &str,
        paths: &[String],
    ) -> Result<ProjectGroup> {
        let paths = group_paths(paths)?;
        let name = group_name(name, &paths)?;
        let _write = WRITES.lock().await;
        let id = id.unwrap_or_else(Uuid::new_v4);
        let key = Key::Group(id.to_string());
        let created_ms =
            read::<GroupRecord>(&self.mesh, &key).map_or_else(now_ms, |old| old.created_ms);
        let record = GroupRecord {
            name: name.clone(),
            paths: paths.clone(),
            created_ms,
        };
        write(&self.mesh, key, Some(&record)).await?;
        Ok(ProjectGroup { id, name, paths })
    }

    pub async fn delete_group(&self, id: Uuid) -> Result<()> {
        let _write = WRITES.lock().await;
        let key = Key::Group(id.to_string());

        if self.mesh.get(&key).is_none() {
            return Err(FleetError::NotFound);
        }

        self.mesh.set(key, None).await?;
        Ok(())
    }
}
