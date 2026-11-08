use super::*;
use flashmind::types::ToolCall;
use serde_json::json;

#[test]
fn a_share_keeps_only_the_message_text() {
    let mut chat = Conversation::new();
    chat.add(ConversationEntry::system_message("Location: home"));
    chat.add(ConversationEntry::user("check my mail"));
    chat.add(ConversationEntry::assistant_with_tool_calls(
        "",
        vec![ToolCall {
            id: "c1".into(),
            name: "mail".into(),
            arguments: json!({}),
        }],
        Some("thinking".into()),
    ));
    chat.add(ConversationEntry::tool("c1", "secret inbox"));
    chat.add(ConversationEntry::assistant_with_reasoning(
        "Two new emails.",
        Some("more thinking".into()),
    ));
    let shared = shared_conversation(&share_history(&chat));
    let json = serde_json::to_string(&shared).unwrap();
    for private in ["home", "secret", "thinking", "c1"] {
        assert!(!json.contains(private), "{private} leaked into {json}");
    }
    let texts: Vec<_> = shared.entries().iter().map(|e| e.content()).collect();
    assert_eq!(texts, ["check my mail", "Two new emails."]);
}
