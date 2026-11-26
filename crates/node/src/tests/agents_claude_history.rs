use super::*;

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
    assert_eq!(prefix_len(&native, &native[4..]), 4);
    assert_eq!(prefix_len(&native, &native), 0);
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
        user("<beep-context>Restored settings</beep-context>"),
        Entry::new(EntryKind::Compacted, "Conversation compacted".into()),
        user("Continue"),
        agent("Done"),
    ];
    assert_eq!(prefix_len(&native, &replay), 2);
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
    assert_eq!(prefix_len(&native, &[tool]), 2);
}

#[test]
fn replay_text_blocks_match_native_spacing_and_size_limits() {
    let native = vec![user("Original request"), agent("first\n\nsecond")];
    assert_eq!(prefix_len(&native, &[agent(" firstsecond ")]), 1);
    let long = "text ".repeat(2000);
    let native = vec![
        user("Original request"),
        agent(&cut(long.trim(), MAX_ENTRY)),
    ];
    assert_eq!(prefix_len(&native, &[agent(&long)]), 1);
}

#[test]
fn an_empty_replay_keeps_available_native_history() {
    let native = vec![user("Original request"), agent("Original reply")];
    assert_eq!(prefix_len(&native, &[]), native.len());
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
    assert_eq!(prefix_len(&native, &replay), 1);
}

#[test]
fn pages_reach_the_first_record_of_a_large_transcript() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut file = std::io::BufWriter::new(File::create(&path).unwrap());
    let padding = "x".repeat(3500);
    for index in 0..5100 {
        writeln!(
            file,
            r#"{{"type":"user","message":{{"content":"Prompt {index}"}},"padding":"{padding}"}}"#
        )
        .unwrap();
    }
    file.flush().unwrap();
    let kept = Transcripts::new(1);
    let page = entries(&path, 2, None, &kept).unwrap().excerpt;
    assert_eq!(page.earlier, 5098);
    let first = entries(&path, 1, Some(1), &kept).unwrap().excerpt;
    assert_eq!(first.entries[0].text, "Prompt 0");
    assert_eq!(first.earlier, 0);
}
