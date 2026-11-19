use super::items::{completed, replayed};
use serde_json::json;

#[test]
fn a_proposed_plan_shows_as_its_text_live_and_replayed() {
    let plan = json!({"type": "plan", "id": "p1", "text": "1. Read the tests\n2. Fix them"});
    let shown = json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "1. Read the tests\n2. Fix them"}});

    assert_eq!(completed(&plan, true), Some(shown.clone()));
    // Streamed as it was written, its completion adds nothing.
    assert_eq!(completed(&plan, false), None);
    assert_eq!(replayed(&plan), vec![shown]);
}

#[test]
fn a_replayed_prompt_keeps_its_pictures() {
    let prompt = json!({"type": "userMessage", "id": "u1", "content": [
        {"type": "text", "text": "What is this?"},
        {"type": "image", "url": "data:image/png;base64,iVBORw0KGgo="},
        {"type": "image", "url": "https://example.com/cat.png"},
    ]});

    assert_eq!(
        replayed(&prompt),
        vec![
            json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "What is this?"}}),
            json!({"sessionUpdate": "user_message_chunk", "content": {"type": "image", "mimeType": "image/png", "data": "iVBORw0KGgo="}}),
            json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "[@image](https://example.com/cat.png)"}}),
        ]
    );
    // A picture alone still shows.
    let alone = json!({"type": "userMessage", "id": "u2", "content": [{"type": "image", "url": "data:image/jpeg;base64,/9j/"}]});
    assert_eq!(replayed(&alone).len(), 1);
}
