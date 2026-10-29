//! When a scope of the user's memories needs curating. Curation costs a
//! model call, so it runs only for a scope that shows a need: it grew by
//! `GROWTH` memories since it was last curated, new memories keep landing
//! close to existing ones without being duplicates (which is also how
//! contradictions look, "Lives in Lisbon" beside "Lives in Porto"), recalls
//! keep coming back with many close hits, or one of its memories ended and
//! its lasting part should be kept. The hourly check reads the memories and
//! the per-scope state, two queries and no model call.

use super::DUPLICATE;
use crate::store::{MemoryRow, Scope, Scored};
use chrono::NaiveDateTime;
use std::collections::HashMap;

/// A memory this close to an existing one in its scope, but not a
/// duplicate, counts as a `Signal::Close` (cosine 0.75).
pub(crate) const NEAR: f32 = 0.645;
/// Memories a scope gains since its last curation that make it due.
const GROWTH: usize = 25;
/// Close memories stored since the last curation that make a scope due.
const CLOSE_PILE: u32 = 3;
/// Crowded recalls since the last curation that make a scope due.
const CROWDED_PILE: u32 = 3;
/// Hits in one scope scoring at least `CROWDED_SCORE` that make a recall
/// crowded. Search scores add up to 0.3 for shared words, so this is close
/// in meaning and in words.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Need {
    Ended,
    Close,
    Crowded,
    Grew,
}

impl Need {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Need::Ended => "ended",
            Need::Close => "close",
            Need::Crowded => "crowded",
            Need::Grew => "grew",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopeNeed {
    pub(crate) scope: Scope,
    pub(crate) need: Need,
}

/// When a memory's days ended by `now`: the start of the day after its
/// last day.
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

/// The scopes of `rows` that need curating, the user's own first, then the
/// biggest.
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
mod tests {
    use super::*;
    use crate::{protocol::MemorySource, store::Validity};
    use chrono::NaiveDate;

    fn row(id: usize, scope: &Scope) -> MemoryRow {
        MemoryRow {
            id: id.to_string(),
            content: format!("Fact {id}."),
            category: "work".into(),
            source: MemorySource::Learned {},
            updated_at: "2026-10-01 08:00:00".into(),
            valid: Validity::default(),
            origin: None,
            core: false,
            scope: scope.clone(),
        }
    }

    fn rows(count: usize, scope: &Scope) -> Vec<MemoryRow> {
        (0..count).map(|id| row(id, scope)).collect()
    }

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn curated(at_text: &str, size: usize) -> ScopeState {
        ScopeState {
            curated_at: Some(at(at_text)),
            size,
            ..ScopeState::default()
        }
    }

    fn machine() -> Scope {
        Scope::Machine(iroh::SecretKey::generate().public())
    }

    #[test]
    fn nothing_is_due_just_because_time_passed() {
        let user = rows(10, &Scope::User);
        let states = HashMap::from([(Scope::User, curated("2026-01-01 00:00:00", 9))]);
        assert!(curation_needs(&user, &states, at("2026-10-29 12:00:00")).is_empty());
    }

    #[test]
    fn a_scope_is_due_once_it_grows_by_a_step() {
        let now = at("2026-10-29 12:00:00");
        let state = HashMap::from([(Scope::User, curated("2026-10-01 00:00:00", 30))]);
        assert!(curation_needs(&rows(GROWTH + 29, &Scope::User), &state, now).is_empty());
        assert_eq!(
            curation_needs(&rows(GROWTH + 30, &Scope::User), &state, now),
            [ScopeNeed {
                scope: Scope::User,
                need: Need::Grew,
            }]
        );
        // A scope never curated is due at its first step.
        assert_eq!(
            curation_needs(&rows(GROWTH, &Scope::User), &HashMap::new(), now).len(),
            1
        );
        assert!(curation_needs(&rows(1, &Scope::User), &HashMap::new(), now).is_empty());
    }

    #[test]
    fn close_and_crowded_signals_pile_up_per_scope() {
        let now = at("2026-10-29 12:00:00");
        let mac = machine();
        let mut all = rows(5, &Scope::User);
        all.extend(rows(5, &mac));
        let mut states = HashMap::from([
            (Scope::User, curated("2026-10-01 00:00:00", 5)),
            (mac.clone(), curated("2026-10-01 00:00:00", 5)),
        ]);
        if let Some(state) = states.get_mut(&mac) {
            state.close = CLOSE_PILE - 1;
        }
        assert!(curation_needs(&all, &states, now).is_empty());
        if let Some(state) = states.get_mut(&mac) {
            state.close = CLOSE_PILE;
        }
        if let Some(state) = states.get_mut(&Scope::User) {
            state.crowded = CROWDED_PILE;
        }
        assert_eq!(
            curation_needs(&all, &states, now),
            [
                ScopeNeed {
                    scope: Scope::User,
                    need: Need::Crowded,
                },
                ScopeNeed {
                    scope: mac,
                    need: Need::Close,
                },
            ]
        );
    }

    #[test]
    fn a_memory_that_ended_since_the_last_curation_is_due_once() {
        let mut user = rows(3, &Scope::User);
        user[0].valid.until = NaiveDate::from_ymd_opt(2026, 10, 20);
        let before = HashMap::from([(Scope::User, curated("2026-10-15 00:00:00", 3))]);
        let after = HashMap::from([(Scope::User, curated("2026-10-21 03:00:00", 3))]);
        // Not over yet on its last day.
        assert!(curation_needs(&user, &before, at("2026-10-20 23:00:00")).is_empty());
        let now = at("2026-10-21 08:00:00");
        assert_eq!(curation_needs(&user, &before, now)[0].need, Need::Ended);
        assert!(curation_needs(&user, &after, now).is_empty());
    }

    #[test]
    fn the_user_scope_comes_first_then_the_biggest() {
        let now = at("2026-10-29 12:00:00");
        let (small, big) = (machine(), Scope::project("beep").unwrap());
        let mut all = rows(GROWTH, &small);
        all.extend(rows(GROWTH + 10, &big));
        all.extend(rows(GROWTH, &Scope::User));
        let order: Vec<Scope> = curation_needs(&all, &HashMap::new(), now)
            .into_iter()
            .map(|need| need.scope)
            .collect();
        assert_eq!(order, [Scope::User, big, small]);
    }

    #[test]
    fn a_recall_is_crowded_when_many_hits_sit_close_in_one_scope() {
        let mac = machine();
        let hit = |id, scope: &Scope, score| Scored {
            row: row(id, scope),
            score,
        };
        let mut found: Vec<Scored> = (0..CROWDED_HITS)
            .map(|id| hit(id, &mac, CROWDED_SCORE + 0.01))
            .collect();
        found.extend((0..CROWDED_HITS).map(|id| hit(id, &Scope::User, CROWDED_SCORE - 0.01)));
        assert_eq!(crowded_scopes(&found), [mac]);
        found.truncate(CROWDED_HITS - 1);
        assert!(crowded_scopes(&found).is_empty());
    }
}
