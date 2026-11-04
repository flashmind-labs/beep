//! Superseded memory versions, owned via `history_of` (not `user`) so lookups never see them; capped by age and `MAX_VERSIONS`.

use super::{CORE, Fact, Meta, USER, from_record, meta_map, now_text, row_from};
use crate::{
    account::user::UserKey,
    store::{MemoryRow, Store},
};
use anyhow::Result;
use chrono::{Duration, Utc};
use flashmind::memory::MemoryRecord;

/// The owner of a superseded entry, in place of `user`.
const HISTORY_OF: &str = "history_of";
/// The id of the memory that replaced an entry.
const SUPERSEDED_BY: &str = "superseded_by";
/// `YYYY-MM-DD HH:MM:SS` in UTC of when an entry was replaced.
const SUPERSEDED_AT: &str = "superseded_at";
/// How long a superseded entry is kept.
const HISTORY_KEPT_DAYS: i64 = 180;
/// Most superseded entries kept per memory; older ones go first.
const MAX_VERSIONS: usize = 5;

/// An earlier version of a memory.
#[derive(Debug)]
pub struct Superseded {
    /// The memory as it was, its id the entry's own.
    pub row: MemoryRow,
    /// `YYYY-MM-DD HH:MM:SS` in UTC of when it was replaced.
    pub superseded_at: String,
}

fn history_expiry() -> i64 {
    (Utc::now() + Duration::days(HISTORY_KEPT_DAYS)).timestamp()
}

/// Metadata of `row` as history superseded by `by`, with no owner or profile flag.
fn history_pairs(
    user: &UserKey,
    row: &MemoryRow,
    by: &str,
    superseded_at: &str,
) -> Vec<(&'static str, String)> {
    let mut pairs: Vec<(&'static str, String)> = Meta::of(user, row, &row.updated_at)
        .pairs()
        .into_iter()
        .filter(|(key, _)| *key != USER && *key != CORE)
        .collect();
    pairs.push((HISTORY_OF, user.as_str().to_owned()));
    pairs.push((SUPERSEDED_BY, by.to_owned()));
    pairs.push((SUPERSEDED_AT, superseded_at.to_owned()));
    pairs
}

fn superseded_from(record: MemoryRecord) -> Superseded {
    let superseded_at = meta_map(&record.meta)
        .get(SUPERSEDED_AT)
        .unwrap_or(&"")
        .to_string();
    Superseded {
        row: from_record(record),
        superseded_at,
    }
}

/// Whether a fact changes what a memory says, its text or its days.
fn changes(row: &MemoryRow, fact: &Fact<'_>) -> bool {
    row.content != fact.content || fact.valid.is_some_and(|valid| valid != row.valid)
}

impl Store {
    /// `update_memory` that keeps the old text as history; returns whether the memory exists.
    pub(crate) async fn replace_memory(
        &self,
        user: &UserKey,
        id: &str,
        fact: Fact<'_>,
    ) -> Result<bool> {
        let Some(before) = self.revise(user, id, fact.clone()).await? else {
            return Ok(false);
        };
        if changes(&before, &fact) {
            self.supersede(user, &before, id).await?;
        }
        Ok(true)
    }

    /// Keeps `row` as it was, superseded by the memory `by`.
    pub(super) async fn supersede(&self, user: &UserKey, row: &MemoryRow, by: &str) -> Result<()> {
        let mut builder = self
            .memories
            .store(&row.content)
            .expires_at(history_expiry());
        for (key, value) in history_pairs(user, row, by, &now_text()) {
            builder = builder.meta(key, &value);
        }
        builder.await?;
        self.trim_history(user, by).await
    }

    /// Turns memory `id`, covered by `by`, into history of `by` with its own history; returns whether it existed.
    pub(crate) async fn retire_memory(&self, user: &UserKey, id: &str, by: &str) -> Result<bool> {
        let Some(record) = self.owned_memory(user, id).await? else {
            return Ok(false);
        };
        let now = now_text();
        let row = from_record(record);
        self.relink(user, &row, by, &now).await?;
        for earlier in self.history_records(user, id).await? {
            let superseded_at = meta_map(&earlier.meta)
                .get(SUPERSEDED_AT)
                .unwrap_or(&now.as_str())
                .to_string();
            let row = row_from(earlier.id, earlier.content, &earlier.meta);
            self.relink(user, &row, by, &superseded_at).await?;
        }
        self.trim_history(user, by).await?;
        Ok(true)
    }

    /// Rewrites the metadata of the record `row` so it is history of `by`.
    async fn relink(
        &self,
        user: &UserKey,
        row: &MemoryRow,
        by: &str,
        superseded_at: &str,
    ) -> Result<()> {
        let pairs = history_pairs(user, row, by, superseded_at);
        let pairs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.memories
            .update(&row.id, None, Some(&pairs), Some(Some(history_expiry())))
            .await?;
        Ok(())
    }

    async fn history_records(&self, user: &UserKey, id: &str) -> Result<Vec<MemoryRecord>> {
        Ok(self
            .memories
            .records()
            .filter(HISTORY_OF, user.as_str())
            .filter(SUPERSEDED_BY, id)
            .limit(MAX_VERSIONS * 4)
            .await?)
    }

    /// The earlier versions of the user's memory `id`, newest first.
    pub(crate) async fn memory_history(&self, user: &UserKey, id: &str) -> Result<Vec<Superseded>> {
        let mut history: Vec<Superseded> = self
            .history_records(user, id)
            .await?
            .into_iter()
            .map(superseded_from)
            .collect();
        history.sort_by(|a, b| b.superseded_at.cmp(&a.superseded_at));
        Ok(history)
    }

    /// Deletes all but the newest `MAX_VERSIONS` entries of `id`.
    async fn trim_history(&self, user: &UserKey, id: &str) -> Result<()> {
        for old in self
            .memory_history(user, id)
            .await?
            .into_iter()
            .skip(MAX_VERSIONS)
        {
            self.memories.delete(&old.row.id).await?;
        }
        Ok(())
    }

    /// Deletes the history of the user's memory `id`.
    pub(super) async fn forget_history(&self, user: &UserKey, id: &str) -> Result<()> {
        self.memories
            .delete_matching(&[(HISTORY_OF, user.as_str()), (SUPERSEDED_BY, id)])
            .await?;
        Ok(())
    }

    /// Deletes all of a user's memory history; returns the entry count.
    pub(super) async fn forget_all_history(&self, user: &UserKey) -> Result<usize> {
        Ok(self
            .memories
            .delete_matching(&[(HISTORY_OF, user.as_str())])
            .await?)
    }
}
