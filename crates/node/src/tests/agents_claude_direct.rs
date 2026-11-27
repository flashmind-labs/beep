use super::{ask, cli, updates};
use agent_client_protocol::v1::schema::{
    CreateElicitationResponse, ElicitationAction, SessionId, SessionUpdate, StopReason,
};
use serde_json::{Value, json};

fn questions() -> Value {
    json!({"questions": [
        {"question": "Which drinks?", "multiSelect": true, "options": [{"label": "Tea"}, {"label": "Coffee"}]},
        {"question": "Which cup?", "multiSelect": false, "options": [{"label": "Mug"}, {"label": "Glass"}]},
    ]})
}

fn reply(content: Value) -> Result<CreateElicitationResponse, agent_client_protocol::Error> {
    Ok(serde_json::from_value(json!({"action": "accept", "content": content})).unwrap())
}

#[test]
fn answers_join_picks_as_claude_code_does_and_keep_notes() {
    let input = questions();
    let (_, asked) = ask::form(&SessionId::new("s"), "t1", &input).unwrap();
    let content = json!({
        "question_0": ["Tea", "Oolong, cold"],
        "question_1": "Mug",
        "question_1_custom": "a big one",
    });

    let decision = ask::answered(reply(content), input.clone(), &asked);

    assert_eq!(decision["behavior"], "allow");
    let updated = &decision["updatedInput"];
    assert_eq!(
        updated["answers"]["Which drinks?"],
        r#"Tea, "Oolong, cold""#
    );
    assert_eq!(updated["answers"]["Which cup?"], "Mug");
    assert_eq!(updated["annotations"]["Which cup?"]["notes"], "a big one");
    assert_eq!(updated["questions"], input["questions"]);
}

#[test]
fn a_declined_form_answers_nothing_and_a_cancelled_one_refuses() {
    let input = questions();
    let (_, asked) = ask::form(&SessionId::new("s"), "t1", &input).unwrap();

    let declined = Ok(CreateElicitationResponse::new(ElicitationAction::Decline));
    let decision = ask::answered(declined, input.clone(), &asked);
    assert_eq!(decision["updatedInput"]["answers"], json!({}));

    let cancelled = Ok(CreateElicitationResponse::new(ElicitationAction::Cancel));
    assert_eq!(ask::answered(cancelled, input, &asked)["behavior"], "deny");
}

#[test]
fn results_end_prompts_as_claude_codes_adapter_ends_them() {
    let result = |fields: Value| {
        let mut result = json!({"type": "result", "subtype": "success", "is_error": false});
        result
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        result
    };

    assert_eq!(
        updates::stop(&result(json!({})), false),
        Ok(StopReason::EndTurn)
    );
    assert_eq!(
        updates::stop(&result(json!({})), true),
        Ok(StopReason::Cancelled)
    );
    let interrupted = json!({"subtype": "error_during_execution", "is_error": true, "terminal_reason": "aborted_streaming"});
    assert_eq!(
        updates::stop(&result(interrupted), false),
        Ok(StopReason::Cancelled)
    );
    let long = json!({"stop_reason": "max_tokens"});
    assert_eq!(
        updates::stop(&result(long), false),
        Ok(StopReason::MaxTokens)
    );
    let login = json!({"is_error": true, "result": "Not logged in · Please run /login"});
    assert_eq!(
        updates::stop(&result(login), false),
        Err("Not logged in · Please run /login".to_owned())
    );
    let turns = json!({"subtype": "error_max_turns"});
    assert_eq!(
        updates::stop(&result(turns), false),
        Ok(StopReason::MaxTurnRequests)
    );

    let woke = result(json!({"origin": {"kind": "task-notification"}}));
    assert!(!updates::ends_prompt(&woke));
    assert!(updates::ends_prompt(&result(json!({}))));
}

#[test]
fn a_transcript_replays_what_was_said_and_done() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let lines = [
        json!({"type": "user", "message": {"role": "user", "content": "Fix the tests"}}),
        json!({"type": "user", "isMeta": true, "message": {"role": "user", "content": "<skill text>"}}),
        json!({"type": "assistant", "message": {"content": [
            {"type": "thinking", "thinking": "Run them first."},
            {"type": "text", "text": "Running them."},
            {"type": "tool_use", "id": "t1", "name": "Edit", "input": {"file_path": "/srv/project/a.rs", "old_string": "a", "new_string": "b"}},
        ]}}),
        json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": "edited", "is_error": true}]}}),
        json!({"type": "assistant", "isSidechain": true, "message": {"content": [{"type": "text", "text": "a subagent"}]}}),
    ];
    let text: Vec<String> = lines.iter().map(Value::to_string).collect();
    std::fs::write(&path, text.join("\n")).unwrap();

    let replayed = updates::replay(&path, false).unwrap();

    let kinds: Vec<&str> = replayed
        .iter()
        .map(|update| match update {
            SessionUpdate::UserMessageChunk(_) => "user",
            SessionUpdate::AgentThoughtChunk(_) => "thought",
            SessionUpdate::AgentMessageChunk(_) => "agent",
            SessionUpdate::ToolCall(_) => "call",
            SessionUpdate::ToolCallUpdate(_) => "result",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["user", "thought", "agent", "call", "result"]);
    let SessionUpdate::ToolCall(call) = &replayed[3] else {
        unreachable!()
    };
    assert_eq!(call.title, "Edit /srv/project/a.rs");
    assert_eq!(call.content.len(), 1, "the edit carries its diff");
}

#[test]
fn replay_marks_a_compaction_summary_from_its_record() {
    let record = json!({"type": "user", "isCompactSummary": true, "isMeta": true,
        "message": {"content": "Saved context in another language"}});
    let replay = updates::message(&record, true);
    let [SessionUpdate::UserMessageChunk(chunk)] = replay.as_slice() else {
        panic!("expected the summary");
    };
    assert_eq!(chunk.meta.as_ref().unwrap()["isCompactSummary"], true);
    assert!(updates::message(&record, false).is_empty());
}

/// The home assistant's session switches off Claude Code's subagent tools
/// in its `_meta`, which the direct bridge passes as one flag (#1127).
#[test]
fn a_session_meta_switches_off_tools_in_one_flag() {
    let meta = json!({"claudeCode": {"options": {"disallowedTools": ["Agent", "Task"], "perTaskStopAffordance": true}}});
    let disallowed = cli::disallowed_tools(meta.as_object());

    assert_eq!(disallowed, ["Agent", "Task"]);
    assert_eq!(
        cli::off_tools(false, &disallowed).as_deref(),
        Some("Agent,Task,AskUserQuestion")
    );
    assert_eq!(
        cli::off_tools(true, &disallowed).as_deref(),
        Some("Agent,Task")
    );
    assert_eq!(cli::off_tools(true, &[]), None);
    assert!(
        cli::disallowed_tools(
            json!({"claudeCode": {"options": {"disallowedTools": "Agent"}}}).as_object()
        )
        .is_empty()
    );
    assert!(cli::disallowed_tools(None).is_empty());
}
