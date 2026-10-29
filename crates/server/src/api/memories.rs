//! Memories and skills.

use super::{Reply, found};
use crate::{
    profile,
    protocol::*,
    router::Cx,
    skills,
    store::{Scope, SkillPatch, SkillRow},
};
use chrono::{DateTime, SecondsFormat, Utc};
use std::collections::HashMap;
use uuid::Uuid;

pub(super) async fn list_memories(cx: Cx, _: ListMemories) -> Reply<Vec<Memory>> {
    Ok(cx.user.list_memories().await?)
}

pub(super) async fn list_memory_reads(cx: Cx, _: ListMemoryReads) -> Reply<Vec<MemoryRead>> {
    Ok(profile::memory_reads(&cx.user.memory_rows().await?))
}

/// Machines are named as the store last knew them, so a memory about a
/// machine whose node row is gone has no name.
pub(super) async fn list_memory_scopes(cx: Cx, _: ListMemoryScopes) -> Reply<Vec<ScopedMemory>> {
    let rows = cx.user.memory_rows().await?;
    let machines = rows
        .iter()
        .any(|row| matches!(row.scope, Scope::Machine(_)));
    let names: HashMap<String, String> = if machines {
        cx.user
            .nodes()
            .await?
            .into_iter()
            .map(|node| (node.endpoint, node.name))
            .collect()
    } else {
        HashMap::new()
    };
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let scope = match row.scope {
                Scope::User => return None,
                Scope::Machine(key) => {
                    let id = key.to_string();
                    MemoryScope::Machine {
                        name: names.get(&id).cloned(),
                        id,
                    }
                }
                Scope::Project(name) => MemoryScope::Project { name },
                Scope::Account(app) => MemoryScope::Account { app },
            };
            Some(ScopedMemory { id: row.id, scope })
        })
        .collect())
}

pub(super) async fn delete_memory(cx: Cx, DeleteMemory { id }: DeleteMemory) -> Reply<()> {
    found(cx.user.forget(&id).await?)
}

/// The new text is embedded on the next lookup.
pub(super) async fn update_memory(cx: Cx, UpdateMemory { id, content }: UpdateMemory) -> Reply<()> {
    let content = content.trim();
    if content.is_empty() || content.len() > 4000 {
        return Err(ApiError::bad_request("memory text must be 1 to 4000 bytes"));
    }
    found(cx.user.edit_memory(&id, content).await?)
}

pub(super) async fn clear_memories(cx: Cx, _: ClearMemories) -> Reply<u64> {
    Ok(cx.user.forget_all().await?)
}

fn skill_view(row: SkillRow) -> Skill {
    let time = |at: DateTime<Utc>| at.to_rfc3339_opts(SecondsFormat::Secs, true);
    Skill {
        id: row.id.to_string(),
        name: row.name,
        description: row.description,
        body: row.body,
        group_id: row.group_id.map(|id| id.to_string()),
        group_name: row.group_name,
        enabled: row.enabled,
        uses: u32::try_from(row.uses).unwrap_or(0),
        last_used: row.last_used.map(time),
        updated_at: time(row.updated_at),
        edited: row.edited,
        can_undo: row.can_undo,
    }
}

fn skill_id(id: &str) -> Result<Uuid, ApiError> {
    id.parse().map_err(|_| ApiError::NotFound {})
}

/// `text` trimmed, or a bad request when it is empty or longer than `max`
/// characters.
fn skill_text<'a>(text: &'a str, what: &str, max: usize) -> Result<&'a str, ApiError> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > max {
        return Err(ApiError::bad_request(format!(
            "a skill's {what} must be 1 to {max} characters"
        )));
    }
    Ok(text)
}

pub(super) async fn list_skills(cx: Cx, _: ListSkills) -> Reply<Vec<Skill>> {
    Ok(cx
        .user
        .visible_skills()
        .await?
        .into_iter()
        .map(skill_view)
        .collect())
}

pub(super) async fn update_skill(cx: Cx, req: UpdateSkill) -> Reply<Skill> {
    let id = skill_id(&req.id)?;
    let name = req
        .name
        .as_deref()
        .map(|name| skill_text(name, "name", skills::NAME_MAX))
        .transpose()?;
    let description = req
        .description
        .as_deref()
        .map(|text| skill_text(text, "description", skills::DESCRIPTION_MAX))
        .transpose()?;
    let body = req.body.as_deref().map(str::trim);
    if body.is_some_and(|body| body.is_empty() || body.len() > skills::BODY_MAX) {
        return Err(ApiError::bad_request(format!(
            "a skill's text must be 1 to {} bytes",
            skills::BODY_MAX
        )));
    }
    let change = SkillPatch {
        name,
        description,
        body,
        enabled: req.enabled,
    };
    match cx.user.edit_skill(id, change).await {
        Ok(Some(row)) => Ok(skill_view(row)),
        Ok(None) => Err(ApiError::NotFound {}),
        Err(error) if is_unique_violation(&error) => Err(ApiError::Conflict {
            message: "there is already a skill with that name".into(),
        }),
        Err(error) => Err(error.into()),
    }
}

pub(super) async fn delete_skill(cx: Cx, DeleteSkill { id }: DeleteSkill) -> Reply<()> {
    found(cx.user.delete_skill(skill_id(&id)?).await?)
}

pub(super) async fn undo_skill(cx: Cx, UndoSkill { id }: UndoSkill) -> Reply<Skill> {
    match cx.user.undo_skill(skill_id(&id)?).await {
        Ok(Some(row)) => Ok(skill_view(row)),
        Ok(None) => Err(ApiError::NotFound {}),
        Err(error) if is_unique_violation(&error) => Err(ApiError::Conflict {
            message: "another skill has the name this one had".into(),
        }),
        Err(error) => Err(error.into()),
    }
}

/// Whether `error` is the database refusing a second row with the same key.
fn is_unique_violation(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<sqlx::Error>(),
        Some(sqlx::Error::Database(error)) if error.is_unique_violation()
    )
}
