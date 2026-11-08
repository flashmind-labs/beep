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
