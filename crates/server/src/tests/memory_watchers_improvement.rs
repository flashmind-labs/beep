use super::*;
use crate::{
    memory::watchers::{
        runner::{Runner, Settings, TimingSettings, WatcherSettings},
        testing::{Harness, Scripted},
    },
    store::test_chat,
};
use chrono::TimeDelta;
use flashmind::types::AgentLlmConfig;
use std::{sync::Arc, time::Duration};

#[test]
fn masks_what_looks_like_a_secret() {
    assert_eq!(
        redact("mail ana@example.com, card 4111-1111-1111-1111 and code 482913."),
        "mail [email], card [number] and code [number]."
    );
    assert_eq!(
        redact("key sk-live-abcdef123456 token a1b2c3d4e5f6g7h8i9j0k1\nbye"),
        "key [secret] token [secret]\nbye"
    );
    assert_eq!(
        redact("Meeting on 2026-10-30 at 19:00 in room 12"),
        "Meeting on 2026-10-30 at 19:00 in room 12"
    );
    assert_eq!(redact("call +34600123456"), "call +[number]");
}

#[test]
fn keeps_known_kinds_within_what_was_read() {
    let answer = r#"```json
        {"findings": [
          {"kind": "correction", "summary": "Booked the wrong day; user said 123456789 is their code.",
           "steps": "1. Ask to book padel tomorrow", "cause": "prompt", "cause_detail": "dates",
           "fix": "Resolve relative dates first.", "area": "system prompt", "slug": "Relative dates",
           "first_seq": 1, "last_seq": 99},
          {"kind": "correction", "summary": "Same again", "area": "System Prompt", "slug": "relative-dates"},
          {"kind": "rant", "summary": "Not a kind", "slug": "x"},
          {"kind": "loop", "summary": "", "slug": "empty summary"},
          {"kind": "slow", "summary": "Took long", "cause": "weather", "slug": "slow search"}
        ]}
        ```"#;
    let checked = check(answer, 10, 20).unwrap();
    assert_eq!(checked.len(), 2);
    assert_eq!(
        checked[0].fingerprint,
        "correction:system-prompt:relative-dates"
    );
    assert_eq!((checked[0].first_seq, checked[0].last_seq), (10, 20));
    assert!(checked[0].summary.contains("[number]"));
    assert_eq!(checked[1].kind, FindingKind::Slow);
    assert_eq!(checked[1].cause, Cause::Other);
    assert!(check("no json", 1, 2).is_err());
}

#[tokio::test]
async fn a_problem_seen_in_two_chats_counts_twice() {
    let model = Scripted::default().answer(
            "You review conversations",
            r#"{"findings":[{"kind":"could_have_found","summary":"Asked for the user's address instead of reading it from memory.","steps":"1. Ask for a taxi home","cause":"prompt","cause_detail":"recall is not suggested","fix":"Tell Beep to recall places before asking.","area":"memory","slug":"asked for known address","first_seq":0,"last_seq":0}]}"#,
        );
    let h = Harness::new(model).await;
    let (first, second) = (test_chat("taxi-1"), test_chat("taxi-2"));
    h.turn_with_calls(&h.alice, first, "get me a taxi home", 3)
        .await;
    h.turn_with_calls(&h.alice, second, "taxi home please", 3)
        .await;
    let slot = WatcherSettings {
        kind: WatcherKind::Improvement,
        llm: AgentLlmConfig::new("openai:test".parse().unwrap()),
        timeout: Duration::from_secs(30),
    };
    let settings = Settings {
        timing: TimingSettings {
            quiet: Duration::from_secs(60),
            retry: Duration::from_secs(60),
        },
        sweep_every: Duration::from_secs(60),
        lookback: Duration::from_secs(3600),
        concurrency: 2,
        watchers: Vec::new(),
    };
    let mut runner = Runner::new(&h.app, settings, vec![(Arc::new(ImprovementWatcher), slot)]);
    runner.tick(Utc::now() + TimeDelta::minutes(2)).await;
    assert!(runner.drain().await.is_empty());

    let found = h.app.store.findings("alice", None, 10).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].seen, 2);
    assert_eq!(found[0].kind, FindingKind::CouldHaveFound);
    assert_eq!(
        found[0].fingerprint,
        "could_have_found:memory:asked-for-known-address"
    );
    let seen = found[0].sighting.as_ref().expect("seen in alice's chats");
    assert!([first, second].contains(&seen.chat));
    assert!(seen.first_seq > 0, "kept within the messages read");
}
