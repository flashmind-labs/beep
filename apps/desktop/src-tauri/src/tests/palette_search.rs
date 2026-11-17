use super::*;
use beep_proto::AgentStatus;
use std::time::{Duration, UNIX_EPOCH};

fn session(machine: &str, id: &str) -> MachineSession {
    MachineSession {
        id: id.into(),
        machine_id: machine.into(),
        machine_name: "build-box".into(),
        agent: "codex".into(),
        title: None,
        cwd: "/srv/project".into(),
        status: AgentStatus::Idle {},
        doing: None,
        started_at: UNIX_EPOCH,
        last_activity: UNIX_EPOCH + Duration::from_secs(1),
        external: false,
        question: None,
        subagents: 0,
        model: None,
        assistant: false,
    }
}

fn found(machine: &str, session: &str, activity: u64) -> Found {
    Found {
        machine: machine.into(),
        hit: SearchHit {
            session: session.into(),
            activity,
            texts: vec![format!("retry in {session}")],
        },
    }
}

#[test]
fn hits_of_listed_sessions_come_newest_first_with_an_excerpt() {
    let sessions = [session("build-box", "old"), session("gpu-box", "new")];
    let hits = vec![
        found("build-box", "old", 1),
        found("gpu-box", "new", 2),
        found("gpu-box", "unlisted", 3),
        found("other-box", "old", 4),
    ];

    let shown = entries(hits, &sessions, &[], "retry");
    let snippets: Vec<_> = shown.iter().map(|entry| entry.snippet.as_deref()).collect();
    assert_eq!(snippets, [Some("retry in new"), Some("retry in old")]);
}

#[test]
fn excerpt_includes_a_late_match() {
    let text = format!(
        "{} Add a retry button. {}",
        "earlier ".repeat(200),
        "later ".repeat(200)
    );
    let found = excerpt(&text, &["retry".into()]).expect("a match");
    assert!(found.contains("retry button"));
    assert!(found.starts_with('…') && found.ends_with('…'));
    assert!(found.len() < 180);
    assert_eq!(
        excerpt("修正 接続を再試行", &["再試行".into()]).as_deref(),
        Some("修正 接続を再試行")
    );
}

#[test]
fn snippet_is_one_excerpt_without_mentions() {
    let texts = [
        "nothing here".to_owned(),
        r#"the focus is on the input @"/path/to/image 31.png" and @src/main.rs, ask @alice"#
            .to_owned(),
    ];
    assert_eq!(
        snippet(&texts, "Focus input").as_deref(),
        Some("the focus is on the input and ask @alice")
    );
    assert_eq!(snippet(&texts, "image"), None);
}
