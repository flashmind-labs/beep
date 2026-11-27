use super::super::limits::MAX_ENTRIES;
use super::*;
use beep_proto::node::agent::SessionInfo;
use serde_json::json;
use std::sync::Arc;

fn live() -> Live {
    Live::new(
        SessionInfo {
            cwd: "/srv/project".into(),
            ..Default::default()
        },
        Arc::default(),
        Arc::default(),
    )
}

fn apply(live: &mut Live, update: Value) {
    live.apply_v2(&serde_json::from_value(update).unwrap());
}

#[test]
fn interleaved_v2_chunks_and_replacements_keep_their_entries() {
    let mut live = live();
    for (id, text) in [("one", "hello "), ("two", "second"), ("one", "world")] {
        apply(
            &mut live,
            json!({"sessionUpdate":"agent_message_chunk","messageId":id,"content":{"type":"text","text":text}}),
        );
    }
    let (entries, _) = live.transcript(10);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].text, "hello world");
    assert_eq!(entries[1].text, "second");
    assert_eq!(live.info.last_message, "second");
    apply(
        &mut live,
        json!({"sessionUpdate":"agent_message","messageId":"one","content":[{"type":"text","text":"replacement"}]}),
    );
    apply(
        &mut live,
        json!({"sessionUpdate":"agent_message","messageId":"one"}),
    );
    assert_eq!(live.transcript(10).0[0].text, "replacement");
    apply(
        &mut live,
        json!({"sessionUpdate":"agent_message","messageId":"one","content":null}),
    );
    assert!(live.transcript(10).0[0].text.is_empty());
}

#[test]
fn local_v2_prompt_echoes_reconcile_by_id_before_or_after_acceptance() {
    for early in [false, true] {
        let mut live = live();
        live.prompted("go");
        live.v2_submitted();
        let echo = json!({"sessionUpdate":"user_message","messageId":"user-one","content":[{"type":"text","text":"go"}]});
        if early {
            apply(&mut live, echo.clone());
        }
        live.v2_accepted(v2::MessageId::new("user-one"));
        if !early {
            apply(&mut live, echo);
        }
        assert_eq!(live.transcript(10).0.len(), 1);
        assert_eq!(live.transcript(10).0[0].text, "go");
    }
}

#[test]
fn tool_upserts_create_once_and_clear_fields_without_losing_the_entry() {
    let mut live = live();
    apply(
        &mut live,
        json!({"sessionUpdate":"tool_call_update","toolCallId":"tool-one","title":"Run","status":"in_progress","content":[{"type":"content","content":{"type":"text","text":"old output"}}]}),
    );
    let first = live.transcript(10).0[0].clone();
    assert!(first.text.contains("old output"));
    apply(
        &mut live,
        json!({"sessionUpdate":"tool_call_update","toolCallId":"tool-one","status":"completed","content":null}),
    );
    let (entries, _) = live.transcript(10);
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].text.contains("old output"));
    assert_eq!(entries[0].kind, EntryKind::Tool);
}

#[test]
fn terminal_snapshots_replace_and_chunks_append() {
    let mut live = live();
    apply(
        &mut live,
        json!({"sessionUpdate":"terminal_update","terminalId":"terminal-one","command":"test-command","output":{"data":"Zmlyc3Q="}}),
    );
    apply(
        &mut live,
        json!({"sessionUpdate":"terminal_output_chunk","terminalId":"terminal-one","data":"IHNlY29uZA=="}),
    );
    assert!(live.transcript(10).0[0].text.ends_with("first second"));
    apply(
        &mut live,
        json!({"sessionUpdate":"terminal_update","terminalId":"terminal-one","output":{"data":"cmVwbGFjZW1lbnQ="}}),
    );
    assert!(live.transcript(10).0[0].text.ends_with("replacement"));
    assert_eq!(live.transcript(10).0.len(), 1);
}

#[test]
fn unknown_updates_are_ignored_and_message_storage_is_bounded() {
    let mut live = live();
    apply(
        &mut live,
        json!({"sessionUpdate":"future_update","newField":true}),
    );
    for id in 0..MAX_ENTRIES + 20 {
        apply(
            &mut live,
            json!({"sessionUpdate":"agent_message_chunk","messageId":id.to_string(),"content":{"type":"text","text":"hello"}}),
        );
    }
    assert_eq!(live.transcript(MAX_ENTRIES).0.len(), MAX_ENTRIES);
    assert!(live.v2.messages.len() <= MAX_ENTRIES + 64);
}

#[test]
fn metadata_only_echo_does_not_turn_the_first_chunk_into_an_append() {
    let mut live = live();
    live.prompted("go");
    live.v2_submitted();
    live.v2_accepted(v2::MessageId::new("user-one"));
    apply(
        &mut live,
        json!({"sessionUpdate":"user_message","messageId":"user-one"}),
    );
    apply(
        &mut live,
        json!({"sessionUpdate":"user_message_chunk","messageId":"user-one","content":{"type":"text","text":"go"}}),
    );
    assert_eq!(live.transcript(10).0[0].text, "go");
}

#[test]
fn v2_diffs_keep_patch_text_and_changed_paths() {
    let mut live = live();
    apply(
        &mut live,
        json!({"sessionUpdate":"tool_call_update","toolCallId":"edit-one","title":"Edit","kind":"edit","status":"completed","content":[{
            "type":"diff", "changes":[{"operation":"modify","path":"/srv/project/notes.txt"}],
            "patch":{"format":"git_patch","text":"@@ -1 +1 @@\n-old\n+new"}
        }]}),
    );
    let entries = live.transcript(10).0;
    assert!(entries[0].text.contains("+new"));
    assert!(
        live.info
            .state
            .files_touched
            .iter()
            .any(|file| file.ends_with("notes.txt"))
    );
}

#[test]
fn terminal_chunks_preserve_utf8_split_between_chunks() {
    let mut live = live();
    for data in ["ww==", "qQ=="] {
        apply(
            &mut live,
            json!({"sessionUpdate":"terminal_output_chunk","terminalId":"terminal-one","data":data}),
        );
    }
    assert_eq!(live.transcript(10).0[0].text, "é");
}

#[test]
fn repeated_v2_child_tool_snapshots_keep_one_step_in_the_parent() {
    let mut live = live();
    apply(
        &mut live,
        json!({"sessionUpdate":"tool_call_update","toolCallId":"parent","title":"Task","rawInput":{"description":"Inspect files"}}),
    );
    apply(
        &mut live,
        json!({"sessionUpdate":"tool_call_update","toolCallId":"child","title":"Read notes","status":"pending","_meta":{"claudeCode":{"parentToolUseId":"parent"}}}),
    );
    apply(
        &mut live,
        json!({"sessionUpdate":"tool_call_update","toolCallId":"child","status":"in_progress"}),
    );
    apply(
        &mut live,
        json!({"sessionUpdate":"tool_call_content_chunk","toolCallId":"child","content":{"type":"content","content":{"type":"text","text":"notes"}}}),
    );
    apply(
        &mut live,
        json!({"sessionUpdate":"tool_call_update","toolCallId":"child","status":"completed"}),
    );

    let parent = &live.tools[&v1::ToolCallId::new("parent")];
    assert_eq!(parent.steps, [v1::ToolCallId::new("child")]);
    assert_eq!(live.steps_of(parent).len(), 1);
    assert_eq!(live.transcript(10).0.len(), 1);
}
