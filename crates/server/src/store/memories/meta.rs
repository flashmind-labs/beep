//! The metadata stored with a memory, and rows read from it.

use super::{
    super::{MemoryRow, Origin, UNFILED, Validity, format_date, parse_date},
    CATEGORY, CORE, ENDED_KEPT_DAYS, MAX_QUOTE_CHARS, ORIGIN, QUOTE, SCOPE, SOURCE, Scope, Scored,
    UPDATED, USER, VALID_FROM, VALID_UNTIL,
};
use crate::{account::user::UserKey, protocol::MemorySource};
use chrono::{Days, Utc};
use flashmind::memory::{MemoryRecord, MemorySearchResult};
use std::collections::HashMap;

pub(super) const UPDATED_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

pub(super) fn now_text() -> String {
    Utc::now().format(UPDATED_FORMAT).to_string()
}

/// When the store may delete a memory: a while after its days are over.
pub(super) fn expiry(valid: &Validity) -> Option<i64> {
    let kept = valid
        .until?
        .checked_add_days(Days::new(ENDED_KEPT_DAYS + 1))?;
    Some(kept.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}

/// The stored metadata of a memory.
pub(super) struct Meta<'a> {
    pub(super) user: &'a str,
    pub(super) category: &'a str,
    pub(super) source: MemorySource,
    pub(super) valid: Validity,
    pub(super) origin: Option<&'a Origin>,
    pub(super) core: bool,
    pub(super) updated: &'a str,
    pub(super) scope: &'a Scope,
}

impl<'a> Meta<'a> {
    /// The metadata of `row`, kept by `user` and last changed at `updated`.
    pub(super) fn of(user: &'a UserKey, row: &'a MemoryRow, updated: &'a str) -> Self {
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

    pub(super) fn pairs(&self) -> Vec<(&'static str, String)> {
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
pub(super) fn meta_map(meta: &[(String, String)]) -> HashMap<&str, &str> {
    meta.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
}

pub(super) fn row_from(id: String, content: String, meta: &[(String, String)]) -> MemoryRow {
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

pub(super) fn from_record(record: MemoryRecord) -> MemoryRow {
    row_from(record.id, record.content, &record.meta)
}

pub(super) fn from_result(result: MemorySearchResult) -> Option<Scored> {
    Some(Scored {
        score: result.score,
        row: row_from(result.id?, result.content, &result.meta),
    })
}

pub(super) fn owned_by(record: &MemoryRecord, user: &UserKey) -> bool {
    meta_map(&record.meta).get(USER) == Some(&user.as_str())
}
