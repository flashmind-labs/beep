//! Memories, as the window's Memories view lists, edits and clears them,
//! with what each is about and the reads that added them.

use crate::{beep::Service, error::Result};
use beep_proto::{
    ClearMemories, DeleteMemory, ListMemories, ListMemoryReads, ListMemoryScopes, Memory,
    MemoryRead, MemoryScope, UpdateMemory,
};
use serde::Serialize;

/// A memory as the bar shows it.
#[derive(Serialize)]
pub struct MemoryView {
    id: String,
    content: String,
    category: String,
    /// `user`, `learned`, `email` or `slack`.
    source: &'static str,
    updated: String,
    valid_from: Option<String>,
    valid_until: Option<String>,
    quote: Option<String>,
}

impl From<Memory> for MemoryView {
    fn from(m: Memory) -> Self {
        Self {
            id: m.id,
            content: m.content,
            category: m.category,
            source: m.source.as_str(),
            updated: m.updated,
            valid_from: m.valid_from,
            valid_until: m.valid_until,
            quote: m.quote,
        }
    }
}

#[tauri::command]
pub async fn list_memories(beep: Service<'_>) -> Result<Vec<MemoryView>> {
    let memories = beep.call(ListMemories {}).await?;
    Ok(memories.into_iter().map(MemoryView::from).collect())
}

/// What a memory is about when it is not the user.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ScopeView {
    /// A machine by its key, with its name while the machine is known.
    Machine {
        id: String,
        name: Option<String>,
    },
    Project {
        name: String,
    },
    /// One of the user's accounts, by app, such as `github`.
    Account {
        app: String,
    },
}

impl From<MemoryScope> for ScopeView {
    fn from(scope: MemoryScope) -> Self {
        match scope {
            MemoryScope::Machine { id, name } => Self::Machine { id, name },
            MemoryScope::Project { name } => Self::Project { name },
            MemoryScope::Account { app } => Self::Account { app },
        }
    }
}

/// A memory's subject, by `Memory::id`.
#[derive(Serialize)]
pub struct MemoryScopeView {
    id: String,
    scope: ScopeView,
}

/// What each memory that is not about the user is about. A memory missing
/// here is about the user.
#[tauri::command]
pub async fn list_memory_scopes(beep: Service<'_>) -> Result<Vec<MemoryScopeView>> {
    let scoped = beep.call(ListMemoryScopes {}).await?;
    Ok(scoped
        .into_iter()
        .map(|memory| MemoryScopeView {
            id: memory.id,
            scope: memory.scope.into(),
        })
        .collect())
}

/// The reads of mail and calendars that added or confirmed memories, newest
/// first.
#[tauri::command]
pub async fn list_memory_reads(beep: Service<'_>) -> Result<Vec<MemoryRead>> {
    beep.call(ListMemoryReads {}).await
}

#[tauri::command]
pub async fn update_memory(beep: Service<'_>, id: String, content: String) -> Result<()> {
    beep.call(UpdateMemory { id, content }).await
}

#[tauri::command]
pub async fn delete_memory(beep: Service<'_>, id: String) -> Result<()> {
    beep.call(DeleteMemory { id }).await
}

#[tauri::command]
pub async fn clear_memories(beep: Service<'_>) -> Result<u64> {
    beep.call(ClearMemories {}).await
}
