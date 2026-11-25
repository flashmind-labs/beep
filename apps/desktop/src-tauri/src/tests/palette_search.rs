use super::*;
use beep_proto::{AgentStatus, node::agent::SearchHit};
use std::time::{Duration, UNIX_EPOCH};

fn session(machine: &str, id: &str, activity: u64) -> MachineSession {
    MachineSession {
        reply: None,
        changes: None,
        id: id.into(),
        machine_id: machine.into(),
        machine_name: "build-box".into(),
        agent: "codex".into(),
        title: None,
        cwd: "/srv/project".into(),
        status: AgentStatus::Idle {},
        doing: None,
        started_at: UNIX_EPOCH,
        last_activity: UNIX_EPOCH + Duration::from_secs(activity),
        external: false,
        question: None,
        subagents: 0,
        model: None,
        assistant: false,
        account: None,
        persona: None,
        place: None,
        continues: None,
    }
}

fn found(machine: &str, session: &str, activity: u64) -> Found {
    Found {
        machine: machine.into(),
        hit: SearchHit {
            session: session.into(),
            activity,
            texts: vec![format!("retry in {session}")],
            metadata: None,
        },
    }
}

#[test]
fn legacy_hits_use_listed_metadata_and_come_newest_first_with_an_excerpt() {
    let sessions = [session("build-box", "old", 1), session("gpu-box", "new", 2)];
    // A copy's activity may be older than the listed one.
    let hits = vec![
        found("gpu-box", "new", 0),
        found("build-box", "old", 9),
        found("gpu-box", "unlisted", 3),
        found("other-box", "old", 4),
    ];

    let machines = [
        ("build-box".into(), "build-box".into()),
        ("gpu-box".into(), "gpu-box".into()),
    ]
    .into();
    let shown = entries(hits, &sessions, &[], &machines, "retry");
    let snippets: Vec<_> = shown.iter().map(|entry| entry.snippet.as_deref()).collect();
    assert_eq!(snippets, [Some("retry in new"), Some("retry in old")]);
}

#[test]
#[cfg(desktop)]
fn copied_history_opens_without_projects_or_listed_sessions() -> anyhow::Result<()> {
    use beep_node::agents::search::IndexCopy;
    use beep_proto::node::agent::{IndexRow, SearchSession};
    use std::path::Path;

    let mut copy = IndexCopy::open(Path::new(":memory:"))?;
    let metadata = SearchSession {
        agent: "codex".into(),
        cwd: "/path/to/unimported".into(),
        title: Some("Archived work".into()),
        started_at: 1,
        managed: None,
    };
    copy.apply(
        "test",
        &[
            IndexRow {
                seq: 1,
                session: "archived".into(),
                text: Some("retry before publishing".into()),
                activity: 2000,
                metadata: None,
            },
            IndexRow {
                seq: 2,
                session: "archived".into(),
                text: Some(String::new()),
                activity: 2000,
                metadata: Some(metadata),
            },
        ],
    )?;
    let hits = copy.find("retry", &[])?;
    let found = hits
        .into_iter()
        .map(|hit| Found {
            machine: "build-box".into(),
            hit,
        })
        .collect();
    let machines = [("build-box".into(), "Build box".into())].into();
    let entries = entries(found, &[], &[], &machines, "retry");
    let [entry] = entries.as_slice() else {
        panic!("expected one historical hit");
    };
    assert_eq!(entry.title, "Archived work");
    assert_eq!(entry.snippet.as_deref(), Some("retry before publishing"));
    let session = serde_json::to_value(&entry.session)?;
    assert_eq!(session["agent"], "codex");
    assert_eq!(session["id"], "archived");
    assert_eq!(session["external"], true);
    Ok(())
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
