//! Memories live in a flashmind `MemoryStore`, next to the main database.
//! Every memory carries its owner in the `user` metadata key, and every call
//! here filters on it, so one user's memories never reach another's.

use super::{MEMORY_CATEGORIES, MemoryRow, Store, Validity, format_date, parse_date};
use crate::{
    protocol::{Memory, MemorySource},
    user::UserKey,
};
use anyhow::{Result, bail};
use async_trait::async_trait;
use chrono::{Days, Utc};
use flashmind::memory::{EmbeddingProvider, MemoryRecord, MemorySearchResult, MemoryStore};
use rusqlite::params;
use std::{collections::HashMap, path::Path, sync::Arc, time::Duration};

/// Memories whose days are over are kept this long, then deleted.
const ENDED_KEPT_DAYS: u64 = 30;
/// Most memories read for one user at once.
const MAX_ROWS: usize = 2000;

const USER: &str = "user";
const CATEGORY: &str = "category";
const SOURCE: &str = "source";
const VALID_FROM: &str = "valid_from";
const VALID_UNTIL: &str = "valid_until";
/// SQLite `datetime('now')` text of the last change.
const UPDATED: &str = "updated";

/// A fact to store or to replace a memory's text with.
#[derive(Clone)]
pub struct Fact<'a> {
    pub content: &'a str,
    /// `None` keeps the current category, or `general` for a new memory.
    pub category: Option<&'a str>,
    pub source: MemorySource,
    /// `None` keeps the current days on an update, or leaves a new memory open.
    pub valid: Option<Validity>,
}

/// A memory found by a search, with its relevance.
pub struct Scored {
    pub row: MemoryRow,
    pub score: f32,
}

fn check_memory(content: &str) -> Result<()> {
    if content.trim().is_empty() || content.len() > 4000 {
        bail!("memory content length out of range");
    }
    Ok(())
}

pub(crate) fn validate_category(category: Option<&str>) -> Result<String> {
    let cat = category.unwrap_or("general");
    if MEMORY_CATEGORIES.contains(&cat) {
        Ok(cat.to_owned())
    } else {
        bail!(
            "invalid category '{}', must be one of: {}",
            cat,
            MEMORY_CATEGORIES.join(", ")
        );
    }
}

fn now_text() -> String {
    Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// When the store may delete a memory: a while after its days are over.
fn expiry(valid: &Validity) -> Option<i64> {
    let kept = valid
        .until?
        .checked_add_days(Days::new(ENDED_KEPT_DAYS + 1))?;
    Some(kept.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}

/// The stored metadata of a memory.
struct Meta<'a> {
    user: &'a str,
    category: &'a str,
    source: MemorySource,
    valid: Validity,
    updated: &'a str,
}

impl Meta<'_> {
    fn pairs(&self) -> Vec<(&'static str, String)> {
        let mut pairs = vec![
            (USER, self.user.to_owned()),
            (CATEGORY, self.category.to_owned()),
            (SOURCE, self.source.as_str().to_owned()),
            (UPDATED, self.updated.to_owned()),
        ];
        if let Some(from) = format_date(self.valid.from) {
            pairs.push((VALID_FROM, from));
        }
        if let Some(until) = format_date(self.valid.until) {
            pairs.push((VALID_UNTIL, until));
        }
        pairs
    }
}

/// A record's metadata by key.
fn meta_map(meta: &[(String, String)]) -> HashMap<&str, &str> {
    meta.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
}

fn row_from(id: String, content: String, meta: &[(String, String)]) -> MemoryRow {
    let meta = meta_map(meta);
    let date = |key| parse_date(meta.get(key).map(|v| v.to_string()));
    MemoryRow {
        id,
        content,
        category: meta.get(CATEGORY).unwrap_or(&"general").to_string(),
        source: MemorySource::parse(meta.get(SOURCE).unwrap_or(&"user")),
        updated_at: meta.get(UPDATED).unwrap_or(&"").to_string(),
        valid: Validity {
            from: date(VALID_FROM),
            until: date(VALID_UNTIL),
        },
    }
}

fn from_record(record: MemoryRecord) -> MemoryRow {
    row_from(record.id, record.content, &record.meta)
}

fn from_result(result: MemorySearchResult) -> Option<Scored> {
    Some(Scored {
        score: result.score,
        row: row_from(result.id?, result.content, &result.meta),
    })
}

fn owned_by(record: &MemoryRecord, user: &UserKey) -> bool {
    meta_map(&record.meta).get(USER) == Some(&user.as_str())
}

/// Embeds a text as the words it contains, hashed into a few dimensions, so
/// tests get meaning-like closeness without a network call.
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
    pub(super) async fn open_memories(
        path: &Path,
        embedder: Arc<dyn EmbeddingProvider>,
    ) -> Result<MemoryStore> {
        Ok(MemoryStore::connect(&path.with_extension("memories.db"), embedder).await?)
    }

    /// Moves memories from the old `memories` table into the memory store,
    /// keeping their embeddings when they fit. Each row is deleted once
    /// copied, so a failed start picks up where it stopped.
    pub(super) async fn move_legacy_memories(&self) -> Result<()> {
        let rows = self
            .db
            .call::<_, _, rusqlite::Error>(|db| {
                let mut stmt = db.prepare(
                    "SELECT user_id, id, content, category, source, updated_at, valid_from, \
                     valid_until, embedding FROM memories",
                )?;
                stmt.query_map([], |r| {
                    let blob: Option<Vec<u8>> = r.get(8)?;
                    Ok(LegacyMemory {
                        user: r.get(0)?,
                        id: r.get(1)?,
                        content: r.get(2)?,
                        category: r.get(3)?,
                        source: r.get(4)?,
                        updated: r.get(5)?,
                        valid: Validity {
                            from: parse_date(r.get(6)?),
                            until: parse_date(r.get(7)?),
                        },
                        embedding: blob.map(|b| {
                            b.as_chunks::<4>()
                                .0
                                .iter()
                                .map(|c| f32::from_le_bytes(*c))
                                .collect()
                        }),
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
            })
            .await?;
        if rows.is_empty() {
            return Ok(());
        }
        let dims = self.memories.embedder().dimensions();
        let mut moved = 0;
        for row in rows {
            let meta = Meta {
                user: &row.user,
                category: &row.category,
                source: MemorySource::parse(&row.source),
                valid: row.valid,
                updated: &row.updated,
            };
            let mut builder = self
                .memories
                .store(&row.content)
                .expires_at_opt(expiry(&row.valid));
            for (key, value) in meta.pairs() {
                builder = builder.meta(key, &value);
            }
            if let Some(embedding) = row.embedding.filter(|e| e.len() == dims) {
                builder = builder.embedding(embedding);
            }
            if let Err(error) = builder.await {
                log::warn!("unable to move a memory, retrying on next start: {error}");
                continue;
            }
            let (user, id) = (row.user, row.id);
            self.db
                .call::<_, _, rusqlite::Error>(move |db| {
                    db.execute(
                        "DELETE FROM memories WHERE user_id=? AND id=?",
                        params![user, id],
                    )
                })
                .await?;
            moved += 1;
        }
        log::info!("moved {moved} memories to the memory store");
        Ok(())
    }

    /// Stores a new memory and returns its id.
    pub(crate) async fn add_memory(&self, user: &UserKey, fact: Fact<'_>) -> Result<String> {
        check_memory(fact.content)?;
        let category = validate_category(fact.category)?;
        let valid = fact.valid.unwrap_or_default();
        let updated = now_text();
        let meta = Meta {
            user: user.as_str(),
            category: &category,
            source: fact.source,
            valid,
            updated: &updated,
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

    /// Rewrites a memory the user owns. Returns whether it exists.
    async fn rewrite(
        &self,
        user: &UserKey,
        id: &str,
        content: &str,
        change: impl FnOnce(&mut MemoryRow),
    ) -> Result<bool> {
        check_memory(content)?;
        let Some(record) = self.owned_memory(user, id).await? else {
            return Ok(false);
        };
        let mut row = from_record(record);
        change(&mut row);
        let updated = now_text();
        let meta = Meta {
            user: user.as_str(),
            category: &row.category,
            source: row.source,
            valid: row.valid,
            updated: &updated,
        };
        let pairs = meta.pairs();
        let pairs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.memories
            .update(
                &row.id,
                Some(content),
                Some(&pairs),
                Some(expiry(&row.valid)),
            )
            .await?;
        Ok(true)
    }

    /// Replaces a memory's text, and its days when given. A memory the user
    /// stated stays stated. Returns whether it exists.
    pub(crate) async fn update_memory(
        &self,
        user: &UserKey,
        id: &str,
        fact: Fact<'_>,
    ) -> Result<bool> {
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
        })
        .await
    }

    /// Replaces only a memory's text, as the user does from the Memories
    /// screen; it then counts as stated by the user.
    pub(crate) async fn edit_memory(
        &self,
        user: &UserKey,
        id: &str,
        content: &str,
    ) -> Result<bool> {
        self.rewrite(user, id, content, |row| row.source = MemorySource::User {})
            .await
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

    /// The user's memories that best match `query`, by meaning and words.
    pub(crate) async fn search_memories(
        &self,
        user: &UserKey,
        query: &str,
        category: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Scored>> {
        let mut search = self
            .memories
            .search(query)
            .filter(USER, user.as_str())
            .limit(limit);
        if let Some(category) = category {
            search = search.filter(CATEGORY, category);
        }
        Ok(search.await?.into_iter().filter_map(from_result).collect())
    }

    /// The user's memory closest in meaning to `text`, if it scores at least
    /// `threshold`.
    pub(crate) async fn closest_memory(
        &self,
        user: &UserKey,
        text: &str,
        threshold: f32,
    ) -> Result<Option<MemoryRow>> {
        let found = self
            .memories
            .similar(text)
            .filter(USER, user.as_str())
            .threshold(threshold)
            .limit(1)
            .await?;
        Ok(found
            .into_iter()
            .next()
            .and_then(from_result)
            .map(|s| s.row))
    }

    pub(crate) async fn forget(&self, user: &UserKey, id: &str) -> Result<bool> {
        let Some(record) = self.owned_memory(user, id).await? else {
            return Ok(false);
        };
        self.memories.delete(&record.id).await?;
        Ok(true)
    }

    /// Deletes every memory of a user and returns how many there were.
    pub(crate) async fn forget_all(&self, user: &UserKey) -> Result<u64> {
        let deleted = self
            .memories
            .delete_matching(&[(USER, user.as_str())])
            .await?;
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
            })
            .collect())
    }

    /// Deletes memories whose days ended a while ago and returns how many.
    pub async fn purge_ended_memories(&self) -> Result<u64> {
        Ok(self.memories.delete_expired().await? as u64)
    }

    /// Whether the user's memories changed since they were last curated,
    /// at most once per `every`.
    pub(crate) async fn curation_due(&self, user: &UserKey, every: Duration) -> Result<bool> {
        let Some(changed) = self
            .memory_rows(user)
            .await?
            .into_iter()
            .map(|row| row.updated_at)
            .max()
        else {
            return Ok(false);
        };
        let id = user.as_str().to_owned();
        let every = format!("-{} seconds", every.as_secs());
        Ok(self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT NOT EXISTS (SELECT 1 FROM memory_curation WHERE user_id = ?1)
                         OR EXISTS (SELECT 1 FROM memory_curation WHERE user_id = ?1
                             AND curated_at <= datetime('now', ?2) AND curated_at < ?3)",
                    params![id, every, changed],
                    |r| r.get(0),
                )
            })
            .await?)
    }

    pub(crate) async fn mark_curated(&self, user: &UserKey) -> Result<()> {
        let user = user.as_str().to_owned();
        self.db
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO memory_curation (user_id, curated_at) VALUES (?, datetime('now'))
                     ON CONFLICT(user_id) DO UPDATE SET curated_at=excluded.curated_at",
                    [user],
                )
            })
            .await?;
        Ok(())
    }
}

/// A row of the old `memories` table.
struct LegacyMemory {
    user: String,
    id: String,
    content: String,
    category: String,
    source: String,
    updated: String,
    valid: Validity,
    embedding: Option<Vec<f32>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn ended_memories_expire_after_the_kept_days() {
        assert_eq!(expiry(&Validity::default()), None);
        let valid = Validity {
            from: None,
            until: Some(date("2026-01-01")),
        };
        let expected = date("2026-02-01").and_hms_opt(0, 0, 0).unwrap();
        assert_eq!(expiry(&valid), Some(expected.and_utc().timestamp()));
    }
}
