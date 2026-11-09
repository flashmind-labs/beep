use crate::{
    account::user::UserKey,
    protocol::{MessageBody, Update},
    store::{StoredBody, TestDb},
};
use chrono::DateTime;
use flashmind::{
    core::{Conversation, ConversationEntry},
    types::ToolCall,
};
use serde_json::json;
use uuid::Uuid;

const AT: i64 = 1_700_000_000_000;

fn transcript() -> Conversation {
    let mut chat = Conversation::new();
    let entries = [
        ConversationEntry::user("Where is lunch?"),
        ConversationEntry::assistant_with_tool_calls(
            "",
            vec![ToolCall {
                id: "a".into(),
                name: "open_maps".into(),
                arguments: json!({}),
            }],
            Some("Look it up.".into()),
        ),
        ConversationEntry::tool("a", "no maps"),
        ConversationEntry::assistant("Here it is."),
        ConversationEntry::user("Thanks"),
        ConversationEntry::assistant("Any time."),
    ];

    for (i, mut entry) in entries.into_iter().enumerate() {
        entry.timestamp = DateTime::from_timestamp_millis(AT + 1000 * i as i64).unwrap();
        chat.add(entry);
    }

    chat
}

#[tokio::test]
async fn old_sessions_and_groups_become_chats_with_messages() {
    let (db, legacy) = TestDb::legacy().await.unwrap();
    let conversation = serde_json::to_string(&transcript()).unwrap();
    for sql in [
        "INSERT INTO sessions (user_id, id, conversation, pinned, updated_at)
             VALUES ('alice', 's1', $1, TRUE, now())",
        "INSERT INTO session_titles (user_id, session_id, title, manual, titled_at)
             VALUES ('alice', 's1', 'Lunch', TRUE, now())",
        "INSERT INTO runs (id, user_id, session_id, request_id, message, status)
             VALUES ('r1', 'alice', 's1', 'q1', 'Where is lunch?', 'completed')",
        "INSERT INTO tool_failures (user_id, session_id, call_id) VALUES ('alice', 's1', 'a')",
        "INSERT INTO turn_models (user_id, session_id, turn_at, model, thinking)
             VALUES ('alice', 's1', 1700000000000, 'z-ai/glm-5.3-flash', 'high')",
        "INSERT INTO memory_scores (user_id, session_id, turn_at, probability)
             VALUES ('alice', 's1', 1700000000000, 0.8)",
        "INSERT INTO files (id, user_id, session_id, name, mime, data, turn_at, width, height)
             VALUES ('p1', 'alice', 's1', 'a.jpg', 'image/jpeg', '\\x01', 1700000000000, 4, 3)",
        "INSERT INTO shares (token, user_id, session_id, title, conversation)
             VALUES ('tok', 'alice', 's1', 'Lunch', '{\"entries\":[]}')",
        "INSERT INTO groups (id, name, invite) VALUES ('g1', 'Trip', 'inv')",
        "INSERT INTO group_members (group_id, user_id, name, creator)
             VALUES ('g1', 'alice', 'Ali', TRUE), ('g1', 'bob', 'Bo', FALSE)",
        "INSERT INTO group_messages (group_id, author_id, author_name, request_id, content)
             VALUES ('g1', 'alice', 'Ali', 'x', 'hi'), ('g1', NULL, 'Beep', NULL, 'hello')",
        "INSERT INTO files (id, user_id, session_id, name, mime, data, group_seq, width, height)
             VALUES ('p2', 'alice', 'g1', 'b.jpg', 'image/jpeg', '\\x02',
                 (SELECT max(seq) FROM group_messages), 4, 3)",
    ] {
        let query = sqlx::query(sql);
        let query = if sql.contains("$1") {
            query.bind(&conversation)
        } else {
            query
        };
        query.execute(&legacy).await.unwrap();
    }
    legacy.close().await;

    let store = db.store().await.unwrap();
    let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
    let sessions = store.sessions(&alice).await.unwrap();
    let [session] = sessions.as_slice() else {
        panic!("unexpected sessions: {sessions:?}");
    };
    assert_eq!(session.title, "Lunch");
    assert!(session.pinned);
    let chat: Uuid = session.id.parse().unwrap();

    let messages = store.chat_messages(chat).await.unwrap();
    let [first, tool, reply, second, _] = messages.as_slice() else {
        panic!("unexpected messages: {messages:?}");
    };
    let StoredBody::User { content, turn } = &first.body else {
        panic!("not a user message: {first:?}");
    };
    assert_eq!(content, "Where is lunch?");
    assert_eq!(turn.model, "z-ai/glm-5.3-flash");
    assert_eq!(turn.memory_score, Some(0.8));
    assert_eq!(turn.reasoning, "Look it up.");
    assert!(
        matches!(&tool.body, StoredBody::Tool { name, success: false, .. } if name == "open_maps")
    );
    assert_eq!(reply.turn_id, Some(first.id));
    assert_eq!(second.turn_id, Some(second.id));
    assert_eq!(first.images.len(), 1);

    let context = store.context(&alice, chat).await.unwrap().unwrap();
    assert_eq!(context.conversation.entries().len(), 6);
    assert!(store.share("tok").await.unwrap().is_some());

    let page = store.updates_page(&alice, 0).await.unwrap();
    let chats = page
        .updates
        .iter()
        .filter(|update| matches!(update, Update::Chat { .. }))
        .count();
    let messages = page
        .updates
        .iter()
        .filter(|update| matches!(update, Update::Message { .. }))
        .count();
    assert_eq!((chats, messages), (2, 7));
    assert_eq!(page.last, store.last_update(alice.as_ref()).await.unwrap());

    let page = store.updates_page(&bob, 0).await.unwrap();
    let group_messages: Vec<_> = page
        .updates
        .iter()
        .filter_map(|update| match update {
            Update::Message { message, .. } => Some(message),
            _ => None,
        })
        .collect();
    let [hi, hello] = group_messages.as_slice() else {
        panic!("unexpected log: {page:?}");
    };
    assert!(matches!(&hi.body, MessageBody::Member { content, you: false, .. } if content == "hi"));
    let MessageBody::Beep { content } = &hello.body else {
        panic!("not Beep's: {hello:?}");
    };
    assert_eq!(content, "hello");
    let Some(Update::Chat { chat: group, .. }) = page.updates.first() else {
        panic!("no chat first: {page:?}");
    };
    let pictures = store.group_pictures(group.id).await.unwrap();
    assert_eq!(pictures[0].message, hello.id);

    // Opening again finds nothing left to do.
    drop(store);
    let store = db.store().await.unwrap();
    assert_eq!(store.chat_messages(chat).await.unwrap().len(), 5);
}
