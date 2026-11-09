use super::*;
use flashmind::core::ConversationEntry;
use flashmind::tools::mcp::{McpBlob, McpContent, McpToolCallResult};
use serde_json::json;

fn image(data: &str) -> ContentPart {
    ContentPart::Image {
        media_type: "image/jpeg".into(),
        data: data.into(),
    }
}

/// The data of every attachment the model gets, oldest first.
fn sent(messages: &[Message]) -> Vec<String> {
    messages
        .iter()
        .flat_map(|message| message.parts.iter().flatten())
        .filter_map(|part| match part {
            ContentPart::Image { data, .. } | ContentPart::Document { data, .. } => {
                Some(data.clone())
            }
            _ => None,
        })
        .collect()
}

fn screenshots(count: usize) -> Conversation {
    let mut conversation = Conversation::new();
    conversation.add(ConversationEntry::user("look at my screen"));

    for n in 0..count {
        conversation.add(ConversationEntry::assistant("again"));
        conversation.add(ConversationEntry::tool_with_parts(
            n.to_string(),
            "Screenshot of mac",
            vec![image(&format!("shot {n}"))],
        ));
    }

    conversation
}

#[test]
fn a_tool_attachment_reaches_the_model_in_flashminds_message() {
    let mut messages = screenshots(1).to_messages();
    keep_recent(&mut messages);
    let last = messages.last().expect("a message");
    assert_eq!(last.role, Role::User);
    assert_eq!(last.content, FROM_TOOLS, "flashmind changed its text");
    assert_eq!(sent(&messages), ["shot 0"]);
}

#[test]
fn only_the_newest_attachments_are_sent() {
    let mut messages = screenshots(6).to_messages();
    keep_recent(&mut messages);
    assert_eq!(sent(&messages), ["shot 3", "shot 4", "shot 5"]);
    let notes = messages
        .iter()
        .flat_map(|message| message.parts.iter().flatten())
        .filter(
            |part| matches!(part, ContentPart::Text { text } if text.contains("no longer shown")),
        )
        .count();
    assert_eq!(notes, 3);
}

/// Chats saved before #164 hold a file's base64 in a tool's text; it is never sent again.
#[test]
fn a_saved_file_printed_as_text_is_left_out_of_the_request() {
    let blob = "JVBERi0xLjcKJcfsj6IK".repeat(50_000);
    let output = format!(
        "Resource(EmbeddedResource {{ resource: BlobResourceContents {{ uri: \"x\", \
             mime_type: Some(\"application/pdf\"), blob: \"{blob}\" }} }})"
    );
    let saved = json!({"entries": [
        {"kind": {"type": "user", "data": {"content": "read my statement"}}, "timestamp": "2026-11-01T10:00:00Z"},
        {"kind": {"type": "tool", "data": {"call_id": "1", "output": output}}, "timestamp": "2026-11-01T10:00:01Z"},
    ]});
    let conversation: Conversation = serde_json::from_value(saved).expect("a saved chat");
    let mut messages = conversation.to_messages();
    keep_recent(&mut messages);
    let sent = serde_json::to_string(&messages).expect("it serializes");
    assert!(sent.len() < 1_000, "{} bytes sent", sent.len());
    assert!(sent.contains("KB of binary data left out"), "{sent}");
}

#[test]
fn a_large_pdf_fails_the_tool_call_instead_of_reaching_the_model() {
    let result = McpToolCallResult {
        content: vec![McpContent::Blob(McpBlob {
            uri: Some("fastmail:2026-05-23_Statement.pdf".into()),
            mime_type: Some("application/pdf".into()),
            data: "JVBE".repeat(916 * 1024 / 3),
        })],
        is_error: false,
    }
    .into_tool_result("1");
    assert!(!result.is_success());
    assert!(result.content().iter().all(
            |part| matches!(part, ContentPart::Text { text } if text.starts_with("2026-05-23_Statement.pdf is 916 KB"))
        ));
}

#[test]
fn a_saved_turn_keeps_no_attachment_data() {
    let mut conversation = screenshots(2);
    conversation.add(ConversationEntry::tool_with_parts(
        "pdf",
        "Attached report.pdf",
        vec![ContentPart::Document {
            media_type: "application/pdf".into(),
            filename: "report.pdf".into(),
            data: "cGRm".into(),
        }],
    ));
    drop_attachments(&mut conversation);
    let saved = serde_json::to_string(&conversation).expect("it serializes");
    assert!(
        !saved.contains("shot 0") && !saved.contains("cGRm"),
        "{saved}"
    );
    assert!(sent(&conversation.to_messages()).is_empty());
    let EntryKind::Tool { output, .. } = &conversation.entries()[2].kind else {
        panic!("a tool result");
    };
    assert!(output.starts_with("Screenshot of mac\n[The attachment is no longer shown"));
}
