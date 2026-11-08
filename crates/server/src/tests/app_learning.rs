use super::*;
use chrono::TimeDelta;

fn call(id: &str, name: &str) -> ToolCall {
    ToolCall {
        id: id.to_owned(),
        name: name.to_owned(),
        arguments: serde_json::json!({}),
    }
}

#[test]
fn follow_ups_leave_the_context() {
    let mut conversation = Conversation::new();
    conversation.add(ConversationEntry::user("weather?"));
    conversation.add(ConversationEntry::assistant_with_tool_calls(
        "",
        vec![call("w", "weather")],
        None,
    ));
    conversation.add(ConversationEntry::tool("w", "sunny"));
    conversation.add(ConversationEntry::assistant("Sunny."));
    conversation.add(ConversationEntry::assistant_with_tool_calls(
        "",
        vec![call("s", tools::SUGGEST_REPLIES), call("m", "remember")],
        None,
    ));
    conversation.add(ConversationEntry::tool("s", "{}"));
    conversation.add(ConversationEntry::tool("m", "saved"));
    conversation.add(ConversationEntry::assistant_with_tool_calls(
        "",
        vec![call("t", tools::SUGGEST_REPLIES)],
        None,
    ));
    conversation.add(ConversationEntry::tool("t", "{}"));

    drop_follow_ups(&mut conversation);

    let names: Vec<String> = conversation
        .entries()
        .iter()
        .map(|entry| match &entry.kind {
            EntryKind::Assistant {
                tool_calls: Some(calls),
                ..
            } => calls
                .iter()
                .map(|call| call.name.as_str())
                .collect::<Vec<_>>()
                .join(","),
            EntryKind::Tool { call_id, .. } => format!("result {call_id}"),
            _ => entry.content().to_owned(),
        })
        .collect();
    assert_eq!(
        names,
        [
            "weather?", "weather", "result w", "Sunny.", "remember", "result m"
        ]
    );
}

#[test]
fn a_quiet_pass_keeps_its_calls_but_not_its_text() {
    let since = Utc::now();
    let mut earlier = ConversationEntry::assistant("Go to Isetan.");
    earlier.timestamp = since - TimeDelta::seconds(1);
    let mut conversation = Conversation::new();
    conversation.add(earlier);
    conversation.add(ConversationEntry::system_message("Show the map."));
    conversation.add(ConversationEntry::assistant_with_tool_calls(
        "Opening the map.",
        vec![call("m", "open_maps")],
        None,
    ));
    conversation.add(ConversationEntry::tool("m", "shown"));
    conversation.add(ConversationEntry::assistant("The map is below."));

    drop_text_since(&mut conversation, since);

    let kept: Vec<(String, usize)> = conversation
        .entries()
        .iter()
        .map(|entry| match &entry.kind {
            EntryKind::Assistant {
                content,
                tool_calls,
                ..
            } => (content.clone(), tool_calls.as_ref().map_or(0, Vec::len)),
            _ => (entry.content().to_owned(), 0),
        })
        .collect();
    assert_eq!(
        kept,
        [
            ("Go to Isetan.".to_owned(), 0),
            ("Show the map.".to_owned(), 0),
            (String::new(), 1),
            ("shown".to_owned(), 0),
        ]
    );
}
