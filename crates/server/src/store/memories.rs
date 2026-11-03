//! Memories in a flashmind `MemoryStore`, filtered by the `user` metadata key on every call, each with a `Scope`.

mod curation;
mod history;

pub use history::Superseded;

use super::{
    MEMORY_CATEGORIES, MemoryRow, Origin, Store, UNFILED, Validity, format_date, parse_date,
};
use crate::{
    protocol::{Memory, MemorySource},
    user::UserKey,
};
use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use chrono::{Days, Utc};
use flashmind::memory::{EmbeddingProvider, MemoryRecord, MemorySearchResult};
use iroh::EndpointId;
use std::collections::HashMap;

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

/// What a memory is about; pre-scope memories are about the user.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum Scope {
    #[default]
    User,
    /// A beep-node machine by key, so renaming keeps its memories.
    Machine(EndpointId),
    /// A project, such as a repository, by the name Beep knows it by.
    Project(String),
    /// One of the user's accounts, by app, such as `github`.
    Account(String),
}

impl Scope {
    /// A project scope, its name trimmed and lowercased so "Beep" and "beep" are one.
    pub fn project(name: &str) -> Result<Self> {
        Ok(Self::Project(scope_name(name)?))
    }

    pub fn account(app: &str) -> Result<Self> {
        Ok(Self::Account(scope_name(app)?))
    }

    /// What kind of scope it is, safe to log where its name is not.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Machine(_) => "machine",
            Self::Project(_) => "project",
            Self::Account(_) => "account",
        }
    }

    /// The value kept in the `scope` metadata key, `None` for the user.
    fn stored(&self) -> Option<String> {
        match self {
            Self::User => None,
            Self::Machine(node) => Some(format!("machine:{node}")),
            Self::Project(name) => Some(format!("project:{name}")),
            Self::Account(app) => Some(format!("account:{app}")),
        }
    }

    /// A stored scope; an unreadable one counts as the user's, the widest.
    fn from_stored(text: Option<&str>) -> Self {
        let Some((kind, name)) = text.and_then(|text| text.split_once(':')) else {
            return Self::User;
        };
        match kind {
            "machine" => name.parse().map_or(Self::User, Self::Machine),
            "project" => Self::Project(name.to_owned()),
            "account" => Self::Account(name.to_owned()),
            _ => Self::User,
        }
    }
}

fn scope_name(name: &str) -> Result<String> {
    let name = name.trim().to_lowercase();
    ensure!(
        !name.is_empty() && name.chars().count() <= MAX_SCOPE_NAME && !name.contains(':'),
        "a scope name has 1 to {MAX_SCOPE_NAME} characters and no colon"
    );
    Ok(name)
}

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

const UPDATED_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

fn now_text() -> String {
    Utc::now().format(UPDATED_FORMAT).to_string()
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
    origin: Option<&'a Origin>,
    core: bool,
    updated: &'a str,
    scope: &'a Scope,
}

impl<'a> Meta<'a> {
    /// The metadata of `row`, kept by `user` and last changed at `updated`.
    fn of(user: &'a UserKey, row: &'a MemoryRow, updated: &'a str) -> Self {
        Self {
            user: user.as_str(),
            category: &row.category,
            source: row.source,
            valid: row.valid,
            origin: row.origin.as_ref(),
            core: row.core,
            updated,
            scope: &row.scope,
        }
    }

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
        if self.core {
            pairs.push((CORE, "1".to_owned()));
        }
        if let Some(scope) = self.scope.stored() {
            pairs.push((SCOPE, scope));
        }
        if let Some(origin) = self.origin {
            pairs.push((ORIGIN, origin.reference.clone()));
            if let Some(quote) = &origin.quote {
                pairs.push((QUOTE, quote.chars().take(MAX_QUOTE_CHARS).collect()));
            }
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
    let date = |key| parse_date(meta.get(key).copied());
    MemoryRow {
        id,
        content,
        category: meta.get(CATEGORY).unwrap_or(&UNFILED).to_string(),
        source: MemorySource::parse(meta.get(SOURCE).unwrap_or(&"user")),
        updated_at: meta.get(UPDATED).unwrap_or(&"").to_string(),
        valid: Validity {
            from: date(VALID_FROM),
            until: date(VALID_UNTIL),
        },
        origin: meta.get(ORIGIN).map(|reference| Origin {
            reference: reference.to_string(),
            quote: meta.get(QUOTE).map(|quote| quote.to_string()),
        }),
        core: meta.get(CORE) == Some(&"1"),
        scope: Scope::from_stored(meta.get(SCOPE).copied()),
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

    /// The user's memories best matching `query` by meaning and words, in `scope` when given.
    /// User-scoped memories have no scope key, so extra rows are read and filtered.
    pub(crate) async fn search_memories(
        &self,
        user: &UserKey,
        query: &str,
        category: Option<&str>,
        scope: Option<&Scope>,
        limit: usize,
    ) -> Result<Vec<Scored>> {
        let stored = scope.and_then(Scope::stored);
        let read = if scope == Some(&Scope::User) {
            limit * 3
        } else {
            limit
        };
        let mut search = self
            .memories
            .search(query)
            .filter(USER, user.as_str())
            .limit(read);
        if let Some(category) = category {
            search = search.filter(CATEGORY, category);
        }
        if let Some(stored) = &stored {
            search = search.filter(SCOPE, stored);
        }
        Ok(search
            .await?
            .into_iter()
            .filter_map(from_result)
            .filter(|found| scope.is_none_or(|scope| found.row.scope == *scope))
            .take(limit)
            .collect())
    }

    /// Vector-only candidates for automatic recall, scoped to one user.
    pub(crate) async fn similar_memories(
        &self,
        user: &UserKey,
        query: &str,
        threshold: f32,
        limit: usize,
    ) -> Result<Vec<Scored>> {
        Ok(self
            .memories
            .similar(query)
            .filter(USER, user.as_str())
            .threshold(threshold)
            .limit(limit)
            .await?
            .into_iter()
            .filter_map(from_result)
            .collect())
    }

    /// The user's memory in `scope` closest to `text` scoring at least `threshold`, with its score.
    pub(crate) async fn closest_memory(
        &self,
        user: &UserKey,
        text: &str,
        scope: &Scope,
        threshold: f32,
    ) -> Result<Option<Scored>> {
        let mut similar = self
            .memories
            .similar(text)
            .filter(USER, user.as_str())
            .threshold(threshold)
            .limit(CLOSEST_CANDIDATES);
        let stored = scope.stored();
        if let Some(stored) = &stored {
            similar = similar.filter(SCOPE, stored);
        }
        Ok(similar
            .await?
            .into_iter()
            .filter_map(from_result)
            .find(|found| found.row.scope == *scope))
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
