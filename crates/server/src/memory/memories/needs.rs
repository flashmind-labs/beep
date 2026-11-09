//! When a scope of the user's memories needs curating (growth, near-duplicates, crowded recalls, ended memories).

use super::DUPLICATE;
use crate::store::{MemoryRow, Scope, Scored};
use chrono::NaiveDateTime;
use std::collections::HashMap;

/// A memory this close to another in its scope, not a duplicate, is a `Signal::Close` (cosine 0.75).
pub(crate) const NEAR: f32 = 0.645;
/// Memories a scope gains since its last curation that make it due.
const GROWTH: usize = 25;
/// Close memories stored since the last curation that make a scope due.
const CLOSE_PILE: u32 = 3;
/// Crowded recalls since the last curation that make a scope due.
const CROWDED_PILE: u32 = 3;
/// Hits scoring at least `CROWDED_SCORE` in one scope that make a recall crowded.
const CROWDED_HITS: usize = 5;
const CROWDED_SCORE: f32 = DUPLICATE;
/// Most scopes one hourly check curates; the rest wait for the next.
pub(crate) const MAX_SCOPES_PER_SWEEP: usize = 3;

/// What the check knows about one scope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ScopeState {
    /// When it was last curated, in UTC; `None` when never.
    pub(crate) curated_at: Option<NaiveDateTime>,
    /// Memories it had after its last curation.
    pub(crate) size: usize,
    /// `Signal::Close` seen since.
    pub(crate) close: u32,
    /// `Signal::Crowded` seen since.
    pub(crate) crowded: u32,
}

/// Something that happened to a scope's memories that may call for curation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Signal {
    /// A memory was stored close to one already there.
    Close,
    /// A recall came back with many close hits.
    Crowded,
}

/// Why a scope is curated.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, strum::AsRefStr, strum::Display, strum::IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum Need {
    Ended,
    Close,
    Crowded,
    Grew,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopeNeed {
    pub(crate) scope: Scope,
    pub(crate) need: Need,
}

/// The start of the day after a memory's last day.
fn ended_at(row: &MemoryRow, now: NaiveDateTime) -> Option<NaiveDateTime> {
    row.valid
        .until?
        .succ_opt()?
        .and_hms_opt(0, 0, 0)
        .filter(|ended| *ended <= now)
}

/// Why `rows`, all in one scope, need curating, if they do.
fn need(rows: &[&MemoryRow], state: &ScopeState, now: NaiveDateTime) -> Option<Need> {
    if rows.len() < 2 {
        return None;
    }

    let since = |at: NaiveDateTime| state.curated_at.is_none_or(|curated| at > curated);

    if rows.iter().filter_map(|row| ended_at(row, now)).any(since) {
        return Some(Need::Ended);
    }

    if state.close >= CLOSE_PILE {
        return Some(Need::Close);
    }

    if state.crowded >= CROWDED_PILE {
        return Some(Need::Crowded);
    }
    (rows.len() >= state.size + GROWTH).then_some(Need::Grew)
}

/// The scopes of `rows` that need curating, the user's own first, then the biggest.
pub(crate) fn curation_needs(
    rows: &[MemoryRow],
    states: &HashMap<Scope, ScopeState>,
    now: NaiveDateTime,
) -> Vec<ScopeNeed> {
    let mut scopes: HashMap<&Scope, Vec<&MemoryRow>> = HashMap::new();

    for row in rows {
        scopes.entry(&row.scope).or_default().push(row);
    }

    let unseen = ScopeState::default();
    let mut needs: Vec<(usize, ScopeNeed)> = scopes
        .into_iter()
        .filter_map(|(scope, rows)| {
            let state = states.get(scope).unwrap_or(&unseen);
            let need = need(&rows, state, now)?;
            Some((
                rows.len(),
                ScopeNeed {
                    scope: scope.clone(),
                    need,
                },
            ))
        })
        .collect();
    needs.sort_by(|(a_len, a), (b_len, b)| {
        (b.scope == Scope::User)
            .cmp(&(a.scope == Scope::User))
            .then(b_len.cmp(a_len))
    });
    needs.into_iter().map(|(_, need)| need).collect()
}

/// The scopes in which a recall's `found` memories crowd together.
pub(crate) fn crowded_scopes(found: &[Scored]) -> Vec<Scope> {
    let mut close: HashMap<&Scope, usize> = HashMap::new();

    for hit in found.iter().filter(|hit| hit.score >= CROWDED_SCORE) {
        *close.entry(&hit.row.scope).or_default() += 1;
    }

    close
        .into_iter()
        .filter(|(_, hits)| *hits >= CROWDED_HITS)
        .map(|(scope, _)| scope.clone())
        .collect()
}

#[cfg(test)]
#[path = "../../tests/memory_memories_needs.rs"]
mod tests;
