use super::*;

fn excerpt(entries: Vec<Entry>) -> Excerpt {
    Excerpt {
        entries,
        earlier: 0,
        pictures: Vec::new(),
    }
}

fn user(text: &str) -> Entry {
    Entry::new(EntryKind::User, text.to_owned())
}

fn agent(text: &str) -> Entry {
    Entry::new(EntryKind::Agent, text.to_owned())
}

#[test]
fn repeated_messages_recover_the_prefix_before_the_replayed_suffix() {
    let native = vec![
        user("yes"),
        agent("Done"),
        user("Again"),
        agent("Ready"),
        user("yes"),
        agent("Done"),
    ];
    let recovered = excerpt(native.clone()).before_replay(&native[4..]);
    assert_eq!(recovered.entries, native[..4]);
    assert!(
        excerpt(native.clone())
            .before_replay(&native)
            .entries
            .is_empty()
    );
}

#[test]
fn replayed_compaction_is_not_added_twice() {
    let native = vec![
        user("Original request"),
        agent("Original reply"),
        Entry::new(EntryKind::Compacted, "[conversation compacted]".into()),
        user("Continue"),
        agent("Done"),
    ];
    let replay = vec![
        Entry::new(EntryKind::Other, "Resumed after beep-node restarted".into()),
        Entry::new(EntryKind::Compacted, "Conversation compacted".into()),
        user("Continue"),
        agent("Done"),
    ];
    assert_eq!(
        excerpt(native.clone()).before_replay(&replay).entries,
        native[..2]
    );
}

#[test]
fn a_tool_only_replay_recovers_older_conversation() {
    let mut tool = Entry::new(EntryKind::Tool, "Inspect workspace".into());
    tool.tool = Some(AgentToolCall {
        name: Some("Bash".into()),
        input: Some(r#"{"command":"pwd"}"#.into()),
        ..AgentToolCall::default()
    });
    let native = vec![user("Inspect the project"), agent("Checking"), tool.clone()];
    tool.text = "pwd [completed]".into();
    tool.tool.as_mut().unwrap().result = Some("/srv/project".into());
    assert_eq!(
        excerpt(native.clone()).before_replay(&[tool]).entries,
        native[..2]
    );
}

#[test]
fn replay_text_blocks_match_native_spacing_and_size_limits() {
    let native = vec![user("Original request"), agent("first\n\nsecond")];
    assert_eq!(
        excerpt(native.clone())
            .before_replay(&[agent(" firstsecond ")])
            .entries,
        native[..1]
    );
    let long = "text ".repeat(2000);
    let native = vec![
        user("Original request"),
        agent(&cut(long.trim(), MAX_ENTRY)),
    ];
    assert_eq!(
        excerpt(native.clone())
            .before_replay(&[agent(&long)])
            .entries,
        native[..1]
    );
}

#[test]
fn an_empty_replay_keeps_available_native_history() {
    let native = vec![user("Original request"), agent("Original reply")];
    assert_eq!(excerpt(native.clone()).before_replay(&[]).entries, native);
}

#[test]
fn long_adjacent_blocks_still_locate_the_replayed_prefix() {
    let first = "a".repeat(MAX_ENTRY - 2);
    let second = "b".repeat(10);
    let native = vec![
        user("Original request"),
        agent(&cut(&format!("{first}\n\n{second}"), MAX_ENTRY)),
    ];
    let replay = vec![agent(&format!("{first}{second}"))];
    assert_eq!(
        excerpt(native.clone()).before_replay(&replay).entries,
        native[..1]
    );
}
