//! Skill libraries: named packs of agent skills from Git repositories, which every machine holds, installed globally or in
//! projects. The desktop writes `skill-library/<name>` and `project-skills/<project>`; each
//! node fetches the library at its revision and installs the picked skills.

use crate::{
    Desk, FleetError, Result,
    records::{read, scan, write},
};
use beep_mesh::Key;
use beep_proto::node::mesh_state::{ProjectSkills, SkillLibrary};

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

    /// Saves `library` under `name`, adding it or replacing the one there.
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
        let mut library = self.skill_library(name).ok_or(FleetError::NotFound)?;

        change(&mut library);
        self.set_skill_library(name, &library).await
    }

    /// Removes the library; every machine uninstalls its skills.
    pub async fn remove_skill_library(&self, name: &str) -> Result<()> {
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
