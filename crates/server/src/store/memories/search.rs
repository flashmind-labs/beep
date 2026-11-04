//! Finding memories by meaning: search, similar and closest.

use super::{super::Store, CATEGORY, CLOSEST_CANDIDATES, SCOPE, Scope, Scored, USER, from_result};
use crate::account::user::UserKey;
use anyhow::Result;

impl Store {
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
}
