//! Memories and skills as the user edits them.

use super::User;
use crate::{
    memory::memories::{
        FILE_CONCURRENCY, FILE_QUESTION, FILING, MAX_FILED, file_question_for, languages, profile,
    },
    protocol::Memory,
    store::{Fact, MEMORY_CATEGORIES, MemoryRow, SkillOwner, SkillPatch, SkillRow},
};
use anyhow::Result;
use futures::{StreamExt, stream};
use std::fmt::Write;
use uuid::Uuid;

impl User {
    /// Stores a new memory, filed by Jev unless it has a category, and returns its id.
    pub async fn add_memory(&self, fact: Fact<'_>) -> Result<String> {
        let fact = Fact {
            category: self.category_of(&fact).await,
            ..fact
        };
        self.store.add_memory(&self.key, fact).await
    }

    /// Replaces a memory's text and refiles it; a stated memory stays stated. Whether it exists.
    pub async fn update_memory(&self, id: &str, fact: Fact<'_>) -> Result<bool> {
        let fact = Fact {
            category: self.category_of(&fact).await,
            ..fact
        };
        self.store.update_memory(&self.key, id, fact).await
    }

    /// Replaces a memory's text from the Memories screen; it then counts as stated by the user.
    pub async fn edit_memory(&self, id: &str, content: &str) -> Result<bool> {
        let category = self.file(content).await;
        self.store
            .edit_memory(&self.key, id, content, category)
            .await
    }

    /// The category `fact` came with, or else Jev's.
    pub(super) async fn category_of<'a>(&self, fact: &Fact<'a>) -> Option<&'a str> {
        match fact.category {
            Some(category) => Some(category),
            None => self.file(fact.content).await,
        }
    }

    /// Jev's category for each of `contents` in one request; `None` when Jev is off or failed.
    pub(super) async fn file_each(&self, contents: &[&str]) -> Vec<Option<&'static str>> {
        let Some(decision) = self.decision.as_ref() else {
            return vec![None; contents.len()];
        };
        match contents {
            [] => return Vec::new(),
            [content] => return vec![self.file(content).await],
            _ => {}
        }
        let mut state = String::from("Memories about the user:");
        for (number, content) in contents.iter().enumerate() {
            let _ = write!(state, "\n{}. {content}", number + 1);
        }
        let keys: Vec<String> = (1..=contents.len()).map(|n| n.to_string()).collect();
        let asked: Vec<String> = keys.iter().map(|key| file_question_for(key)).collect();
        let questions: Vec<(&str, &str)> = keys
            .iter()
            .zip(&asked)
            .map(|(key, question)| (key.as_str(), question.as_str()))
            .collect();
        match decision.choose_each(&state, &questions, &FILING).await {
            Ok(filed) => keys.iter().map(|key| filed.get(key).copied()).collect(),
            Err(error) => {
                log::warn!("unable to file memories: {error}; user={self}");
                vec![None; contents.len()]
            }
        }
    }

    /// Jev's category for a memory, `None` when Jev is not set up or failed.
    pub(super) async fn file(&self, content: &str) -> Option<&'static str> {
        let decision = self.decision.as_ref()?;
        match decision.choose(content, FILE_QUESTION, &FILING).await {
            Ok(category) => Some(category),
            Err(error) => {
                log::warn!("unable to file a memory: {error}; user={self}");
                None
            }
        }
    }

    /// Files unfiled or stale-category memories a few at a time; returns how many.
    pub(crate) async fn file_memories(&self) -> Result<usize> {
        if self.decision.is_none() {
            return Ok(0);
        }
        let rows: Vec<MemoryRow> = self
            .memory_rows()
            .await?
            .into_iter()
            .filter(|row| !MEMORY_CATEGORIES.contains(&row.category.as_str()))
            .take(MAX_FILED)
            .collect();
        let filed: Vec<(String, &str)> = stream::iter(rows)
            .map(|row| async move { self.file(&row.content).await.map(|c| (row.id, c)) })
            .buffer_unordered(FILE_CONCURRENCY)
            .filter_map(|filed| async move { filed })
            .collect()
            .await;
        let mut changed = 0;
        for (id, category) in filed {
            changed += usize::from(self.store.set_category(&self.key, &id, category).await?);
        }
        Ok(changed)
    }

    /// Every memory of the user, most recently changed first.
    pub async fn memory_rows(&self) -> Result<Vec<MemoryRow>> {
        self.store.memory_rows(&self.key).await
    }

    /// The memory profile for a chat's context, `None` when it is empty.
    pub async fn memory_profile(&self) -> Result<Option<String>> {
        Ok(profile(&self.memory_rows().await?, self.today().await))
    }

    /// The memory saying which languages the user speaks, if there is one.
    pub async fn spoken_languages(&self) -> Result<Option<String>> {
        let rows = self.memory_rows().await?;
        Ok(languages(&rows, self.today().await).map(str::to_owned))
    }

    /// Deletes a memory; `false` when the user has no memory with `id`.
    pub async fn forget(&self, id: &str) -> Result<bool> {
        self.store.forget(&self.key, id).await
    }

    /// Deletes every memory of a user and returns how many there were.
    pub async fn forget_all(&self) -> Result<u64> {
        self.store.forget_all(&self.key).await
    }

    /// The user's own skills, as Beep reads and learns them.
    pub fn skill_owner(&self) -> SkillOwner {
        SkillOwner::User(self.key.clone())
    }

    /// The user's skills and their groups', for the Skills screen.
    pub async fn visible_skills(&self) -> Result<Vec<SkillRow>> {
        self.store.visible_skills(&self.key).await
    }

    /// Changes one of the user's skills; `None` when there is no such skill.
    pub async fn edit_skill(&self, id: Uuid, change: SkillPatch<'_>) -> Result<Option<SkillRow>> {
        self.store.edit_skill(&self.key, id, change).await
    }

    pub async fn delete_skill(&self, id: Uuid) -> Result<bool> {
        self.store.delete_skill(&self.key, id).await
    }

    /// Puts back the version before the skill's last change.
    pub async fn undo_skill(&self, id: Uuid) -> Result<Option<SkillRow>> {
        self.store.undo_skill(&self.key, id).await
    }

    /// Every memory for the Memories screen, most recently changed first.
    pub async fn list_memories(&self) -> Result<Vec<Memory>> {
        self.store.list_memories(&self.key).await
    }
}
