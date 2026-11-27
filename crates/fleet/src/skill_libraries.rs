//! Git skill libraries and their installation choices. A controller asks one node to inspect
//! Git and publishes the revision for the other machines to install.

use crate::{
    Desk, FleetError, Result,
    records::{WRITES, now_ms, read, scan, write},
};
use beep_mesh::{DeviceKind, Key};
use beep_projects::imports::repo::remote_key;
use beep_proto::node::{
    mesh_state::{LibrarySkill, Pick, ProjectSkills, SkillLibrary, StateValue},
    skill_library::{PROTOCOL, Reply, Request, library_name},
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, time::Duration};

const DAY_MS: u64 = 24 * 60 * 60 * 1000;
const INSPECT_LIMIT: Duration = Duration::from_secs(120);

/// Kept separately so checking an unchanged repository never overwrites installation choices.
#[derive(Serialize, Deserialize)]
struct LibraryCheck {
    url: String,
    revision: String,
    reader: EndpointId,
    checked_ms: u64,
}

impl StateValue for LibraryCheck {}

impl Desk {
    /// Every library, by name.
    pub fn skill_libraries(&self) -> Vec<(String, SkillLibrary)> {
        scan::<SkillLibrary>(&self.mesh, "skill-library/")
            .into_iter()
            .filter_map(|(key, library)| match key {
                Key::SkillLibrary(name) => Some((name, library)),
                _ => None,
            })
            .collect()
    }

    pub fn skill_library(&self, name: &str) -> Option<SkillLibrary> {
        read(&self.mesh, &Key::SkillLibrary(name.to_owned()))
    }

    /// Adds a library at the repository's current revision.
    pub async fn add_skill_library(
        &self,
        url: String,
        global: bool,
    ) -> Result<(String, SkillLibrary)> {
        let (reader, revision, skills) = self.inspect_library(&url, None).await?;
        let library = SkillLibrary {
            url,
            revision,
            skills,
            global: if global { Pick::All } else { Pick::None },
            projects: Default::default(),
            chats: Default::default(),
        };

        let _writing = WRITES.lock().await;
        let base = library_name(&library.url);
        let name = self.free_library_name(&base);
        self.record_library_check(&name, &library, reader).await?;
        self.set_skill_library(&name, &library).await?;
        Ok((name, library))
    }

    /// Reads a library now, even if its daily check already ran.
    pub async fn update_skill_library(&self, name: &str) -> Result<()> {
        let library = self.skill_library(name).ok_or(FleetError::NotFound)?;
        self.refresh_library(name, &library).await
    }

    /// The first online controller running the scheduler checks the libraries due this pass.
    pub async fn check_skill_libraries(&self) -> Result<()> {
        let libraries = self.skill_libraries();
        if libraries.is_empty() {
            return Ok(());
        }

        let marker = Key::SkillLibraryChecker(self.local.to_string());
        if self.mesh.get(&marker).is_none() {
            self.mesh.set_if_new(marker, Vec::new()).await?;
            // Give other controllers a pass to learn who can check, including on older meshes.
            return Ok(());
        }

        if !self.is_library_checker() {
            return Ok(());
        }

        let now = now_ms();
        for (name, library) in libraries {
            let recent = self.library_check(&name, &library).is_some_and(|check| {
                check.revision == library.revision
                    && now
                        .checked_sub(check.checked_ms)
                        .is_some_and(|age| age < DAY_MS)
            });

            if !recent && self.refresh_library(&name, &library).await.is_err() {
                log::warn!("could not update skill library {name}, will retry");
            }
        }

        Ok(())
    }

    /// How long until the soonest library is due its daily check; zero when one is due now,
    /// `None` when there are no libraries.
    pub fn next_library_check_in(&self) -> Option<Duration> {
        let now = now_ms();

        self.skill_libraries()
            .into_iter()
            .map(|(name, library)| {
                let due = self
                    .library_check(&name, &library)
                    .filter(|check| check.revision == library.revision)
                    .map_or(0, |check| check.checked_ms.saturating_add(DAY_MS));

                Duration::from_millis(due.saturating_sub(now))
            })
            .min()
    }

    fn is_library_checker(&self) -> bool {
        self.mesh
            .devices()
            .into_iter()
            .filter(|status| status.online && status.device.kind == DeviceKind::Desktop)
            .map(|status| status.device.id)
            .filter(|id| {
                self.mesh
                    .get(&Key::SkillLibraryChecker(id.to_string()))
                    .is_some()
            })
            .min_by_key(|id| (self.mesh.is_observer(*id), *id))
            == Some(self.local)
    }

    async fn refresh_library(&self, name: &str, before: &SkillLibrary) -> Result<()> {
        let previous = self.library_check(name, before).map(|check| check.reader);
        let (reader, revision, skills) = self.inspect_library(&before.url, previous).await?;

        let _writing = WRITES.lock().await;
        let mut current = self.skill_library(name).ok_or(FleetError::NotFound)?;
        if current.url != before.url || current.revision != before.revision {
            return Err(FleetError::Conflict(
                "The library changed during its update.".into(),
            ));
        }

        if current.revision != revision || current.skills != skills {
            current.revision = revision;
            current.skills = skills;
            self.set_skill_library(name, &current).await?;
        }

        self.record_library_check(name, &current, reader).await
    }

    fn library_check(&self, name: &str, library: &SkillLibrary) -> Option<LibraryCheck> {
        read::<LibraryCheck>(&self.mesh, &Key::SkillLibraryCheck(name.to_owned()))
            .filter(|check| check.url == library.url)
    }

    async fn record_library_check(
        &self,
        name: &str,
        library: &SkillLibrary,
        reader: EndpointId,
    ) -> Result<()> {
        let check = LibraryCheck {
            url: library.url.clone(),
            revision: library.revision.clone(),
            reader,
            checked_ms: now_ms(),
        };
        write(
            &self.mesh,
            Key::SkillLibraryCheck(name.to_owned()),
            Some(&check),
        )
        .await
    }

    /// Prefer the host that could read this repository last time, including private repositories.
    async fn inspect_library(
        &self,
        url: &str,
        previous: Option<EndpointId>,
    ) -> Result<(EndpointId, String, Vec<LibrarySkill>)> {
        let mut online: Vec<_> = self
            .mesh
            .code_hosts()
            .into_iter()
            .filter(|status| status.online)
            .map(|status| status.device.id)
            .collect();
        // Relative paths, file URLs and drive letters refer to the original host.
        let portable = remote_key(url).is_some()
            && !url.starts_with("file:")
            && url.as_bytes().get(1) != Some(&b':');

        if let Some(reader) = previous
            && !portable
        {
            online.retain(|id| *id == reader);
        }

        online.sort_by_key(|id| (Some(*id) != previous, *id != self.local, *id));
        let request = Request::Inspect {
            url: url.to_owned(),
        };
        let mut last_error = FleetError::refused("No machine is online to read it.");

        for machine in online {
            match self.inspect_on(machine, &request).await {
                Ok(Reply::Inspected { revision, skills }) => {
                    return Ok((machine, revision, skills));
                }
                Ok(Reply::Failed { message }) => last_error = FleetError::refused(message),
                Ok(_) => {
                    last_error = FleetError::refused("The machine returned an unexpected reply.")
                }
                Err(error) => last_error = error,
            }
        }

        Err(last_error)
    }

    async fn inspect_on(&self, machine: EndpointId, request: &Request) -> Result<Reply> {
        let client = self.machines.client(machine).await?;
        Ok(client.call_within(INSPECT_LIMIT, PROTOCOL, request).await?)
    }

    fn free_library_name(&self, base: &str) -> String {
        let base = if base.is_empty() { "library" } else { base };
        let taken: BTreeSet<String> = self
            .skill_libraries()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        (1..)
            .map(|n| match n {
                1 => base.to_owned(),
                n => format!("{base}-{n}"),
            })
            .find(|name| !taken.contains(name))
            .unwrap_or_else(|| base.to_owned())
    }

    pub async fn set_skill_library(&self, name: &str, library: &SkillLibrary) -> Result<()> {
        write(
            &self.mesh,
            Key::SkillLibrary(name.to_owned()),
            Some(library),
        )
        .await
    }

    /// Changes the library `name` with `change`.
    pub async fn edit_skill_library(
        &self,
        name: &str,
        change: impl FnOnce(&mut SkillLibrary),
    ) -> Result<()> {
        let _writing = WRITES.lock().await;
        let mut library = self.skill_library(name).ok_or(FleetError::NotFound)?;
        change(&mut library);
        self.set_skill_library(name, &library).await
    }

    /// Removes the library. Every machine uninstalls its skills.
    pub async fn remove_skill_library(&self, name: &str) -> Result<()> {
        let _writing = WRITES.lock().await;
        write::<LibraryCheck>(&self.mesh, Key::SkillLibraryCheck(name.to_owned()), None).await?;
        write::<SkillLibrary>(&self.mesh, Key::SkillLibrary(name.to_owned()), None).await
    }

    pub fn project_skills(&self, project: &str) -> ProjectSkills {
        read(&self.mesh, &Key::ProjectSkills(project.to_owned())).unwrap_or_default()
    }

    pub async fn set_project_skills(&self, project: &str, settings: &ProjectSkills) -> Result<()> {
        write(
            &self.mesh,
            Key::ProjectSkills(project.to_owned()),
            Some(settings),
        )
        .await
    }
}
