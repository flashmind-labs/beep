use super::{curation::*, *};
use crate::{
    memory::memories::curation::NewText,
    protocol::MemorySource,
    store::{MemoryRow, Scope, Validity},
};
use chrono::NaiveDate;
use std::collections::{HashMap, HashSet};

fn row(id: &str, content: &str) -> MemoryRow {
    MemoryRow {
        id: id.into(),
        content: content.into(),
        category: "identity".into(),
        source: MemorySource::Learned {},
        updated_at: "2026-01-01 10:00:00".into(),
        valid: Validity::default(),
        origin: None,
        core: false,
        scope: Scope::User,
    }
}

fn day(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

#[test]
fn curation_keeps_what_the_user_said() {
    let mut stated = row("u1", "Prefers tea.");
    stated.source = MemorySource::User {};
    let learned = row("l1", "Likes tea.");
    let other = row("l2", "Asked about flights once.");
    let mut trip = row("u2", "Is in Rome.");
    trip.source = MemorySource::User {};
    trip.valid.until = Some(day("2026-09-01"));
    let rows: HashMap<&str, &MemoryRow> = [&stated, &learned, &other, &trip]
        .into_iter()
        .map(|r| (r.id.as_str(), r))
        .collect();
    let ops = parse_curation(
        r#"{"ops":[
        {"op":"forget","id":"u1"},
        {"op":"merge","ids":["l1","u1"],"content":"Prefers tea."},
        {"op":"forget","id":"l1"},
        {"op":"forget","id":"missing"},
        {"op":"forget","id":"l2"},
        {"op":"forget","id":"u2"}]}"#,
    )
    .unwrap()
    .ops;
    let steps = plan_curation(ops, &rows, day("2026-10-03"));
    assert_eq!(
        steps,
        vec![
            Step::Rewrite {
                keep: "u1".into(),
                fact: NewText {
                    content: "Prefers tea.".into(),
                    source: MemorySource::User {},
                    valid: Some(Validity::default()),
                },
                drop: vec!["l1".into()],
            },
            Step::Forget("l2".into()),
            Step::Forget("u2".into()),
        ]
    );
}

#[test]
fn a_contradicted_memory_is_superseded_not_forgotten() {
    let old = row("l1", "Works at Acme.");
    let new = row("l2", "Works at Globex.");
    let merged = row("l3", "Started at Globex in May.");
    let mut stated = row("u1", "Lives in Porto.");
    stated.source = MemorySource::User {};
    let moved = row("l4", "Lives in Lisbon.");
    let rows: HashMap<&str, &MemoryRow> = [&old, &new, &merged, &stated, &moved]
        .into_iter()
        .map(|r| (r.id.as_str(), r))
        .collect();
    let ops = parse_curation(
        r#"{"ops":[
        {"op":"supersede","id":"l1","by":"l2"},
        {"op":"merge","ids":["l3","l2"],"content":"Works at Globex since May."},
        {"op":"supersede","id":"u1","by":"l4"},
        {"op":"supersede","id":"missing","by":"l4"}]}"#,
    )
    .unwrap()
    .ops;
    let steps = plan_curation(ops, &rows, day("2026-10-03"));
    assert_eq!(
        steps[0],
        Step::Supersede {
            id: "l1".into(),
            by: "l3".into(),
        },
        "a supersede follows its newer memory into a merge"
    );
    assert_eq!(steps.len(), 2, "a learned memory never overrules the user");
    let core = core_ids(vec!["l1".into()], &steps, &rows);
    assert_eq!(core, HashSet::from(["l3".to_owned()]));
}

#[test]
fn an_ended_memory_rewritten_without_days_lasts() {
    let mut trip = row("t1", "Is in Lisbon.");
    trip.valid = Validity {
        from: Some(day("2026-09-20")),
        until: Some(day("2026-09-27")),
    };
    let mut stay = row("t2", "Is staying with Ana.");
    stay.valid.until = Some(day("2026-10-10"));
    let rows: HashMap<&str, &MemoryRow> = [&trip, &stay]
        .into_iter()
        .map(|r| (r.id.as_str(), r))
        .collect();
    let ops = parse_curation(
        r#"{"ops":[
        {"op":"update","id":"t1","content":"Went to Lisbon in September 2026."},
        {"op":"update","id":"t2","content":"Is staying with Ana, a friend."}]}"#,
    )
    .unwrap()
    .ops;
    let steps = plan_curation(ops, &rows, day("2026-10-03"));
    let valid: Vec<Option<Validity>> = steps
        .into_iter()
        .map(|step| match step {
            Step::Rewrite { fact, .. } => fact.valid,
            Step::Forget(_) | Step::Supersede { .. } | Step::Split { .. } => {
                panic!("unexpected step")
            }
        })
        .collect();
    assert_eq!(valid, vec![Some(Validity::default()), None]);
}

#[test]
fn a_merge_never_cuts_a_fact_short() {
    let mut flight = row("f", "Flies SQ012 to Tokyo.");
    flight.valid = Validity {
        from: Some(day("2026-10-09")),
        until: Some(day("2026-10-09")),
    };
    let mut stay = row("s", "Is staying at a hotel in Tokyo.");
    stay.valid = Validity {
        from: Some(day("2026-10-09")),
        until: Some(day("2026-11-06")),
    };
    let home = row("h", "Lives in Dubai.");
    let rows: HashMap<&str, &MemoryRow> = [&flight, &stay, &home]
        .into_iter()
        .map(|r| (r.id.as_str(), r))
        .collect();
    let merge = |ids: &str| {
        let ops = parse_curation(&format!(
            r#"{{"ops":[{{"op":"merge","ids":{ids},"content":"Flies to Tokyo.",
            "valid_from":"2026-10-09","valid_until":"2026-10-09"}}]}}"#
        ))
        .unwrap()
        .ops;
        plan_curation(ops, &rows, day("2026-10-08"))
    };
    let Step::Rewrite { fact, .. } = &merge(r#"["f","s"]"#)[0] else {
        panic!("expected a rewrite");
    };
    assert_eq!(fact.valid, Some(stay.valid));
    assert!(merge(r#"["s","h"]"#).is_empty());
}

#[test]
fn a_merge_with_mail_in_it_stays_mail() {
    let mut bank = row("b", "Banks with Santander.");
    bank.source = MemorySource::Email {};
    let mut loan = row("l", "Has a mortgage.");
    loan.source = MemorySource::User {};
    let rows: HashMap<&str, &MemoryRow> = [&bank, &loan]
        .into_iter()
        .map(|r| (r.id.as_str(), r))
        .collect();
    let ops = parse_curation(
        r#"{"ops":[{"op":"merge","ids":["l","b"],"content":"Has a mortgage with Santander."}]}"#,
    )
    .unwrap()
    .ops;
    let Step::Rewrite { keep, fact, .. } = &plan_curation(ops, &rows, day("2026-10-03"))[0] else {
        panic!("expected a rewrite");
    };
    assert_eq!(fact.source, MemorySource::Email {});
    assert_eq!(keep, "b");
}
