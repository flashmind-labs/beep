//! A stored memory, its validity and where it came from.

use super::Scope;
use crate::protocol::MemorySource;
use chrono::NaiveDate;

/// The categories Jev files a memory under, with what each holds.
pub(crate) const MEMORY_CATEGORIES: &[&str] = &[
    "identity",
    "people",
    "preference",
    "work",
    "place",
    "plan",
    "routine",
];

/// Category a memory has until Jev files it; the hourly sweep refiles these.
pub(crate) const UNFILED: &str = "unfiled";

/// The days a fact holds, like the dates of a trip. Both open by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Validity {
    pub from: Option<NaiveDate>,
    pub until: Option<NaiveDate>,
}

impl Validity {
    /// Whether the fact no longer holds on `today`.
    pub fn ended(&self, today: NaiveDate) -> bool {
        self.until.is_some_and(|until| until < today)
    }
}

/// Where a memory was picked up: source chat, email or Slack thread, and the quoted words.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Origin {
    /// The session id, email id or Slack thread id, by the memory's source.
    pub reference: String,
    pub quote: Option<String>,
}

/// A stored memory.
#[derive(Debug, Clone)]
pub struct MemoryRow {
    pub id: String,
    pub content: String,
    pub category: String,
    pub source: MemorySource,
    /// `YYYY-MM-DD HH:MM:SS` in UTC.
    pub updated_at: String,
    pub valid: Validity,
    pub origin: Option<Origin>,
    /// Picked by curation as part of the profile every chat starts with.
    pub core: bool,
    pub scope: Scope,
}

impl MemoryRow {
    /// `YYYY-MM-DD` of the last change.
    pub fn updated(&self) -> &str {
        self.updated_at.get(..10).unwrap_or_default()
    }
}
