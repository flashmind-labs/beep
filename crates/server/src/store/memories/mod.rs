//! Memories in a flashmind `MemoryStore`, filtered by the `user` metadata key on every call, each with a `Scope`.

mod meta;
mod scope;
use meta::{Meta, expiry, from_record, from_result, meta_map, now_text, owned_by, row_from};
pub use scope::Scope;

mod search;

mod curation;
mod history;

pub use history::Superseded;

use super::{MEMORY_CATEGORIES, MemoryRow, Origin, Store, UNFILED, Validity, format_date};
use crate::{
    account::user::UserKey,
    protocol::{Memory, MemorySource},
};
use anyhow::{Result, bail};
use async_trait::async_trait;
use flashmind::memory::{EmbeddingProvider, MemoryRecord};

/// Memories whose days are over are kept this long, then deleted.
const ENDED_KEPT_DAYS: u64 = 30;
/// Most memories read for one user at once.
pub(crate) const MAX_ROWS: usize = 2000;

const USER: &str = "user";
const CATEGORY: &str = "category";
const SOURCE: &str = "source";
const VALID_FROM: &str = "valid_from";
const VALID_UNTIL: &str = "valid_until";
const ORIGIN: &str = "origin";
/// Set to `1` on a memory in the profile.
const CORE: &str = "core";
const QUOTE: &str = "quote";
/// Longest quote kept with a memory, in characters.
const MAX_QUOTE_CHARS: usize = 200;
/// `YYYY-MM-DD HH:MM:SS` in UTC of the last change.
const UPDATED: &str = "updated";
/// `Scope::stored`; missing for a memory about the user.
const SCOPE: &str = "scope";
/// How many close memories are read to find the closest in one scope.
const CLOSEST_CANDIDATES: usize = 8;
/// Longest project or account name in a scope.
const MAX_SCOPE_NAME: usize = 100;

/// A fact to store or to replace a memory's text with.
#[derive(Clone)]
pub struct Fact<'a> {
    pub content: &'a str,
    /// `None` keeps the current category, or leaves a new memory unfiled.
    pub category: Option<&'a str>,
    pub source: MemorySource,
    /// `None` keeps the current days on an update, or leaves a new memory open.
    pub valid: Option<Validity>,
    /// `None` keeps the current origin on an update.
    pub origin: Option<Origin>,
    /// `None` keeps the scope on update, or means the user on create.
    pub scope: Option<Scope>,
}

/// A memory found by a search, with its relevance.
pub struct Scored {
    pub row: MemoryRow,
    pub score: f32,
}

fn check_memory(content: &str) -> Result<()> {
    if content.trim().is_empty() || content.chars().count() > 4000 {
        bail!("memory content length out of range");
    }
    Ok(())
}

fn validate_category(category: Option<&str>) -> Result<String> {
    let cat = category.unwrap_or(UNFILED);
    if cat == UNFILED || MEMORY_CATEGORIES.contains(&cat) {
        Ok(cat.to_owned())
    } else {
        bail!(
            "invalid category '{}', must be one of: {}",
            cat,
            MEMORY_CATEGORIES.join(", ")
        );
    }
}

/// Hashed bag-of-words embedder so tests get meaning-like closeness offline.
#[doc(hidden)]
pub struct WordEmbedder;

#[async_trait]
impl EmbeddingProvider for WordEmbedder {
    async fn embed(&self, text: &str) -> flashmind::memory::Result<Vec<f32>> {
        let mut vector = vec![0.0f32; 64];
        for word in text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 3)
        {
            let hash = word
                .to_lowercase()
                .bytes()
                .fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
            vector[hash as usize % 64] += 1.0;
        }
        let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt().max(1.0);
        Ok(vector.into_iter().map(|x| x / norm).collect())
    }

    fn dimensions(&self) -> usize {
        64
    }

    fn name(&self) -> &str {
        "words"
    }
}

impl Store {
    /// Stores a new memory and returns its id.
    pub(crate) async fn add_memory(&self, user: &UserKey, fact: Fact<'_>) -> Result<String> {
        check_memory(fact.content)?;
        let category = validate_category(fact.category)?;
        let valid = fact.valid.unwrap_or_default();
        let updated = now_text();
        let scope = fact.scope.unwrap_or_default();
        let meta = Meta {
            user: user.as_str(),
            category: &category,
            source: fact.source,
            valid,
            origin: fact.origin.as_ref(),
            core: false,
            updated: &updated,
            scope: &scope,
        };
        let mut builder = self
            .memories
            .store(fact.content)
            .expires_at_opt(expiry(&valid));
        for (key, value) in meta.pairs() {
            builder = builder.meta(key, &value);
        }
        Ok(builder.await?)
    }

    /// The user's memory with this id, if it is theirs.
    async fn owned_memory(&self, user: &UserKey, id: &str) -> Result<Option<MemoryRecord>> {
        Ok(self
            .memories
            .get(id)
            .await?
            .filter(|record| owned_by(record, user)))
    }

    /// Rewrites a memory the user owns; returns it as it was, `None` when missing.
    async fn rewrite(
        &self,
        user: &UserKey,
        id: &str,
        content: &str,
        change: impl FnOnce(&mut MemoryRow),
    ) -> Result<Option<MemoryRow>> {
        check_memory(content)?;
        let Some(record) = self.owned_memory(user, id).await? else {
            return Ok(None);
        };
        let before = from_record(record);
        let mut row = before.clone();
        change(&mut row);
        let updated = now_text();
        let pairs = Meta::of(user, &row, &updated).pairs();
        let pairs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.memories
            .update(
                &row.id,
                Some(content),
                Some(&pairs),
                Some(expiry(&row.valid)),
            )
            .await?;
        Ok(Some(before))
    }

    /// Replaces a memory's text (and days when given); a stated memory stays stated. Returns whether it exists.
    pub(crate) async fn update_memory(
        &self,
        user: &UserKey,
        id: &str,
        fact: Fact<'_>,
    ) -> Result<bool> {
        Ok(self.revise(user, id, fact).await?.is_some())
    }

    /// `update_memory` that returns the memory as it was.
    async fn revise(&self, user: &UserKey, id: &str, fact: Fact<'_>) -> Result<Option<MemoryRow>> {
        let category = fact
            .category
            .map(|c| validate_category(Some(c)))
            .transpose()?;
        self.rewrite(user, id, fact.content, |row| {
            if let Some(category) = category {
                row.category = category;
            }
            if row.source != (MemorySource::User {}) {
                row.source = fact.source;
            }
            if let Some(valid) = fact.valid {
                row.valid = valid;
            }
            if let Some(origin) = fact.origin {
                row.origin = Some(origin);
            }
            if let Some(scope) = fact.scope {
                row.scope = scope;
            }
        })
        .await
    }

    /// The user's edit from Memories: replaces text (and category), marks it stated, and keeps the old text as history.
    pub(crate) async fn edit_memory(
        &self,
        user: &UserKey,
        id: &str,
        content: &str,
        category: Option<&str>,
    ) -> Result<bool> {
        let category = category.map(|c| validate_category(Some(c))).transpose()?;
        let before = self
            .rewrite(user, id, content, |row| {
                row.source = MemorySource::User {};
                if let Some(category) = category {
                    row.category = category;
                }
            })
            .await?;
        let Some(before) = before else {
            return Ok(false);
        };
        if before.content != content {
            self.supersede(user, &before, id).await?;
        }
        Ok(true)
    }

    /// Every memory of a user, most recently changed first.
    pub(crate) async fn memory_rows(&self, user: &UserKey) -> Result<Vec<MemoryRow>> {
        let records = self
            .memories
            .records()
            .filter(USER, user.as_str())
            .limit(MAX_ROWS)
            .await?;
        let mut rows: Vec<MemoryRow> = records.into_iter().map(from_record).collect();
        rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(rows)
    }

    /// Puts a memory in or out of the profile, keeping its change date; returns whether it changed.
    pub(crate) async fn set_core(&self, user: &UserKey, id: &str, core: bool) -> Result<bool> {
        self.retag(user, id, |row| row.core = core).await
    }

    /// Recategorizes a memory, keeping its change date; returns whether it changed.
    pub(crate) async fn set_category(
        &self,
        user: &UserKey,
        id: &str,
        category: &str,
    ) -> Result<bool> {
        let category = validate_category(Some(category))?;
        self.retag(user, id, |row| row.category = category).await
    }

    /// Changes a memory's metadata only; returns whether anything changed.
    async fn retag(
        &self,
        user: &UserKey,
        id: &str,
        change: impl FnOnce(&mut MemoryRow),
    ) -> Result<bool> {
        let Some(record) = self.owned_memory(user, id).await? else {
            return Ok(false);
        };
        let mut row = from_record(record);
        let (core, category) = (row.core, row.category.clone());
        change(&mut row);
        if row.core == core && row.category == category {
            return Ok(false);
        }
        let pairs = Meta::of(user, &row, &row.updated_at).pairs();
        let pairs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.memories
            .update(&row.id, None, Some(&pairs), None)
            .await?;
        Ok(true)
    }

    pub(crate) async fn forget(&self, user: &UserKey, id: &str) -> Result<bool> {
        let Some(record) = self.owned_memory(user, id).await? else {
            return Ok(false);
        };
        self.memories.delete(&record.id).await?;
        // Forgetting a memory forgets what it said before too.
        self.forget_history(user, &record.id).await?;
        Ok(true)
    }

    /// Deletes all of a user's memories and history; returns the memory count.
    pub(crate) async fn forget_all(&self, user: &UserKey) -> Result<u64> {
        let deleted = self
            .memories
            .delete_matching(&[(USER, user.as_str())])
            .await?;
        self.forget_all_history(user).await?;
        Ok(deleted as u64)
    }

    /// Every memory for the Memories screen, most recently changed first.
    pub(crate) async fn list_memories(&self, user: &UserKey) -> Result<Vec<Memory>> {
        Ok(self
            .memory_rows(user)
            .await?
            .into_iter()
            .map(|row| Memory {
                updated: row.updated().to_owned(),
                id: row.id,
                content: row.content,
                category: row.category,
                source: row.source,
                valid_from: format_date(row.valid.from),
                valid_until: format_date(row.valid.until),
                quote: row.origin.and_then(|origin| origin.quote),
            })
            .collect())
    }

    /// Deletes memories whose days ended a while ago and returns how many.
    pub async fn purge_ended_memories(&self) -> Result<u64> {
        Ok(self.memories.delete_expired().await? as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_length_counts_characters_not_bytes() {
        assert!(check_memory(&"é".repeat(4000)).is_ok());
        assert!(check_memory(&"é".repeat(4001)).is_err());
        assert!(check_memory(" ").is_err());
    }
}
