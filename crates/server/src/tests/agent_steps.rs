use super::*;
use chrono::Duration;
use flashmind::types::{ReasoningLevel, ToolCall};
use serde_json::json;
use uuid::Uuid;

/// The history of `entries` once saved as steps.
fn saved_history(
    entries: &[ConversationEntry],
    calls: &CallNotes,
    turns: &TurnsByTime,
) -> Vec<HistoryEntry> {
    history(&stored(steps(entries, None, calls, turns).steps, 1))
}

/// `steps` saved as the store does from id `first` on.
fn stored(steps: Vec<Step>, first: i64) -> Vec<StoredMessage> {
    let mut turn_id = None;
    steps
        .into_iter()
        .enumerate()
        .map(|(index, step)| {
            let id = index as i64 + first;

            if step.body.opens_turn() {
                turn_id = Some(id);
            }
            StoredMessage {
                id,
                chat_id: Uuid::nil(),
                turn_id,
                sent_at: step.sent_at,
                author_id: None,
                body: step.body.into(),
                images: Vec::new(),
            }
        })
        .collect()
}

#[test]
fn history_attaches_tools_to_the_turn() {
    let call = |id: &str, name: &str| ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: json!({}),
    };
    let mut entries = [
        ConversationEntry::user("Where is lunch?"),
        ConversationEntry::assistant_with_tool_calls(
            "",
            vec![call("a", "open_maps")],
            Some("Look it up.".into()),
        ),
        ConversationEntry::tool("a", "é".repeat(MAX_TOOL_OUTPUT)),
        ConversationEntry::assistant_with_reasoning("Here it is.", Some("Found one.".into())),
        ConversationEntry::user("Remind me"),
        ConversationEntry::assistant_with_tool_calls("", vec![call("b", "cron_create")], None),
        ConversationEntry::tool("b", "failed"),
    ];
    // Each turn is found by its user message's time, so they must differ.
    entries[4].timestamp = entries[0].timestamp + Duration::seconds(1);
    let asked = entries[0].timestamp.timestamp_millis();
    let history = saved_history(
        &entries,
        &CallNotes {
            failed: HashSet::from(["b".to_owned()]),
            ..CallNotes::default()
        },
        &HashMap::from([(
            asked,
            TurnRecord {
                model: "z-ai/glm-5.3-flash".to_owned(),
                auto_thinking: Some(ReasoningLevel::High),
                reasoning: String::new(),
                memory_score: Some(0.8),
            },
        )]),
    );
    let [
        HistoryEntry::User {
            auto: first_auto, ..
        },
        HistoryEntry::Assistant {
            content: first,
            tools: first_tools,
            memory_score: first_score,
            reasoning: first_reasoning,
            model: first_model,
            ..
        },
        HistoryEntry::User {
            auto: second_auto, ..
        },
        HistoryEntry::Assistant {
            content: second,
            tools: second_tools,
            memory_score: second_score,
            reasoning: second_reasoning,
            model: second_model,
            ..
        },
    ] = history.as_slice()
    else {
        panic!("unexpected history: {history:?}");
    };
    assert_eq!(first, "Here it is.");
    assert_eq!(*first_score, Some(0.8));
    assert_eq!(*second_score, None);
    assert_eq!(first_model, "z-ai/glm-5.3-flash");
    assert!(second_model.is_empty());
    assert_eq!(
        first_auto.as_ref().map(|pick| pick.thinking),
        Some(ReasoningLevel::High)
    );
    assert!(second_auto.is_none());
    assert_eq!(first_reasoning, "Look it up.\n\nFound one.");
    assert!(second_reasoning.is_empty());
    assert_eq!(first_tools[0].name, "open_maps");
    assert!(first_tools[0].success);
    assert_eq!(first_tools[0].output.len(), MAX_TOOL_OUTPUT);
    assert!(second.is_empty());
    assert_eq!(second_tools[0].call_id, "b");
    assert!(!second_tools[0].success);
}

#[test]
fn history_marks_a_routine_turn_as_scheduled() {
    let entries = [
        ConversationEntry::system_message(cron::scheduled_context("Plants")),
        ConversationEntry::user("Remind the user to water the plants"),
        ConversationEntry::assistant("Time to water the plants."),
        ConversationEntry::system_message("Time: Monday 2026-10-05 09:00 +04"),
        ConversationEntry::user("Thanks"),
        ConversationEntry::user("[Scheduled: Old] Remind me to stretch"),
    ];
    let history = saved_history(&entries, &CallNotes::default(), &HashMap::new());
    let [
        HistoryEntry::Scheduled { label, .. },
        HistoryEntry::Assistant { .. },
        HistoryEntry::User { content, .. },
        HistoryEntry::Scheduled { label: old, .. },
    ] = history.as_slice()
    else {
        panic!("unexpected history: {history:?}");
    };
    assert_eq!(label, "Plants");
    assert_eq!(content, "Thanks");
    assert_eq!(old, "Old");
}

#[test]
fn a_routines_notice_shows_alone_after_the_turn_before() {
    let call = ToolCall {
        id: "a".into(),
        name: "open_maps".into(),
        arguments: json!({}),
    };
    let chat = [
        ConversationEntry::user("Where is lunch?"),
        ConversationEntry::assistant_with_tool_calls("", vec![call.clone()], None),
        ConversationEntry::tool("a", "map"),
        ConversationEntry::assistant("Here it is."),
    ];
    let routine = [
        ConversationEntry::system_message(cron::scheduled_context("Reply")),
        ConversationEntry::user("Check whether Sam replied."),
        ConversationEntry::assistant_with_tool_calls(
            "Checking.",
            vec![ToolCall {
                id: "b".into(),
                ..call
            }],
            Some("Look.".into()),
        ),
        ConversationEntry::tool("b", "found"),
        ConversationEntry::assistant("Sam replied: lunch is on."),
    ];
    let notice = routine_notice(
        &routine,
        None,
        steps(&routine, None, &CallNotes::default(), &HashMap::new()),
    );
    assert!(notice.carried_reasoning.is_empty());
    let mut messages = stored(
        steps(&chat, None, &CallNotes::default(), &HashMap::new()).steps,
        1,
    );
    // Saved on its own, the notice has no turn.
    messages.extend(stored(notice.steps, 10));
    let history = history(&messages);
    let [
        HistoryEntry::User { .. },
        HistoryEntry::Assistant {
            content: reply,
            tools,
            ..
        },
        HistoryEntry::Assistant {
            content: told,
            tools: none,
            ..
        },
    ] = history.as_slice()
    else {
        panic!("unexpected history: {history:?}");
    };
    assert_eq!(reply, "Here it is.");
    assert_eq!(tools.len(), 1);
    assert_eq!(told, "Sam replied: lunch is on.");
    assert!(none.is_empty());
}

#[test]
fn a_routines_notice_is_what_it_told_the_user_with_what_it_sent() {
    let call = |id: &str, name: &str| ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: json!({}),
    };
    let opening = [
        ConversationEntry::system_message(cron::scheduled_context("Reply")),
        ConversationEntry::user("Check whether Sam replied."),
        ConversationEntry::assistant_with_tool_calls("", vec![call("i", "send_image")], None),
        ConversationEntry::tool("i", "sent"),
        ConversationEntry::assistant_with_tool_calls("", vec![call("x", "send_file")], None),
        ConversationEntry::tool("x", "Not sent: no such file"),
    ];
    let sign_off = [
        ConversationEntry::assistant_with_tool_calls("", vec![call("e", "end_routine")], None),
        ConversationEntry::tool("e", "ended"),
        ConversationEntry::assistant("Routine ended."),
    ];
    // Written with the call, or right after it.
    let with_call = [
        ConversationEntry::assistant_with_tool_calls(
            "Sam replied: lunch is on.",
            vec![call("n", "notify_user")],
            None,
        ),
        ConversationEntry::tool("n", "sent"),
    ];
    let after_call = [
        ConversationEntry::assistant_with_tool_calls("", vec![call("n", "notify_user")], None),
        ConversationEntry::tool("n", "sent"),
        ConversationEntry::assistant("Sam replied: lunch is on."),
    ];
    let failed = CallNotes {
        failed: HashSet::from(["x".to_owned()]),
        ..CallNotes::default()
    };

    for told in [&with_call[..], &after_call[..]] {
        let routine: Vec<ConversationEntry> = opening
            .iter()
            .chain(told)
            .chain(&sign_off)
            .cloned()
            .collect();
        let notice = routine_notice(
            &routine,
            None,
            steps(&routine, None, &failed, &HashMap::new()),
        );
        let history = history(&stored(notice.steps, 1));
        let [HistoryEntry::Assistant { content, tools, .. }] = history.as_slice() else {
            panic!("unexpected history: {history:?}");
        };
        assert_eq!(content, "Sam replied: lunch is on.");
        let sent: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(sent, ["send_image"]);
    }
}

#[test]
fn steps_before_the_first_opener_carry_their_reasoning() {
    let mut entries = [
        ConversationEntry::user("Earlier"),
        ConversationEntry::assistant_with_reasoning("Waiting on you.", None),
        ConversationEntry::assistant_with_reasoning("Done.", Some("Approved.".into())),
    ];
    entries[2].timestamp = entries[0].timestamp + Duration::seconds(1);
    let since = entries[2].timestamp;
    let out = steps(
        &entries,
        Some(since),
        &CallNotes::default(),
        &HashMap::new(),
    );
    assert_eq!(out.carried_reasoning, "Approved.");
    assert!(out.steps.iter().all(|step| !step.body.opens_turn()));
}
