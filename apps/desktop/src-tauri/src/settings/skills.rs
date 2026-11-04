//! Skills: listing, editing, deleting and undoing a change to one.

use crate::{beep::Service, chat::markdown, error::Result};
use beep_proto::{DeleteSkill, ListSkills, Skill, UndoSkill, UpdateSkill};
use serde::{Deserialize, Serialize};

/// A skill as the bar shows it: the server's fields, and the body drawn as HTML.
#[derive(Serialize)]
pub struct SkillView {
    #[serde(flatten)]
    skill: Skill,
    html: String,
}

impl From<Skill> for SkillView {
    fn from(skill: Skill) -> Self {
        let html = markdown::to_html(&skill.body);
        Self { skill, html }
    }
}

/// The changes the skill editor sends; a field left out stays as it is.
#[derive(Deserialize)]
pub struct SkillChange {
    name: Option<String>,
    description: Option<String>,
    body: Option<String>,
    enabled: Option<bool>,
}

#[tauri::command]
pub async fn list_skills(beep: Service<'_>) -> Result<Vec<SkillView>> {
    let skills = beep.call(ListSkills {}).await?;
    Ok(skills.into_iter().map(SkillView::from).collect())
}

#[tauri::command]
pub async fn update_skill(beep: Service<'_>, id: String, change: SkillChange) -> Result<SkillView> {
    let skill = beep
        .call(UpdateSkill {
            id,
            name: change.name,
            description: change.description,
            body: change.body,
            enabled: change.enabled,
        })
        .await?;
    Ok(skill.into())
}

#[tauri::command]
pub async fn delete_skill(beep: Service<'_>, id: String) -> Result<()> {
    beep.call(DeleteSkill { id }).await
}

/// Puts back the version before the skill's last change.
#[tauri::command]
pub async fn undo_skill(beep: Service<'_>, id: String) -> Result<SkillView> {
    Ok(beep.call(UndoSkill { id }).await?.into())
}
