use super::*;
use beep_proto::{GroupMember, TurnInfo};

fn message(id: i64, body: MessageBody) -> ChatMessage {
    ChatMessage {
        id,
        turn_id: None,
        timestamp: id * 1000,
        body,
    }
}

fn user(id: i64, content: &str) -> ChatMessage {
    in_turn(
        id,
        message(
            id,
            MessageBody::User {
                content: content.into(),
                images: Vec::new(),
                turn: TurnInfo {
                    model: String::new(),
                    auto: None,
                    reasoning: String::new(),
                    memory_score: None,
                },
            },
        ),
    )
}

/// `message` as a step of the turn `turn` opened.
fn in_turn(turn: i64, message: ChatMessage) -> ChatMessage {
    ChatMessage {
        turn_id: Some(turn),
        ..message
    }
}

fn tool(id: i64, name: &str, output: &str) -> ChatMessage {
    message(
        id,
        MessageBody::Tool {
            call_id: format!("c{id}"),
            name: name.into(),
            output: output.into(),
            success: true,
        },
    )
}

fn reply(id: i64, content: &str) -> ChatMessage {
    message(
        id,
        MessageBody::Assistant {
            content: content.into(),
        },
    )
}

fn info(id: Uuid, kind: ChatKind, title: &str) -> ChatInfo {
    ChatInfo {
        id,
        kind,
        title: title.into(),
        pinned: false,
        members: Vec::new(),
        apps_shared: true,
    }
}

fn page(last: i64, updates: Vec<Update>) -> UpdatePage {
    UpdatePage {
        updates,
        last,
        more: false,
        reset: false,
    }
}

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("beep-sync-{name}-{}", Uuid::new_v4()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn tools_and_replies_sit_on_the_turns_last_reply() {
    let messages = [
        user(1, "Where is lunch?"),
        in_turn(1, tool(2, "open_maps", "{}")),
        in_turn(1, reply(3, "Here it is.")),
        in_turn(
            1,
            tool(4, history::SUGGEST_REPLIES, r#"{"replies":["Thanks"]}"#),
        ),
        user(5, "Remind me"),
        in_turn(5, tool(6, "cron_create", "failed")),
    ];
    let turns = turns(&messages);
    let [
        Turn::User { .. },
        Turn::Assistant { tools, replies, .. },
        Turn::User { .. },
        Turn::Assistant {
            html, tools: last, ..
        },
    ] = turns.as_slice()
    else {
        panic!("unexpected turns: {turns:?}");
    };
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "open_maps");
    assert_eq!(replies, &["Thanks"]);
    assert!(html.is_empty());
    assert_eq!(last[0].name, "cron_create");
}

#[test]
fn an_alert_is_left_out_of_the_lists_until_it_is_a_chat() {
    let dir = dir("alerts");
    let (alert, chat) = (Uuid::new_v4(), Uuid::new_v4());
    let chats = Chats::load(dir);
    let mut updates = Vec::new();
    for (at, (id, title, text)) in [
        (alert, "Arrived in Girona", "The hotel opens at 15:00."),
        (chat, "Lunch", "The hotel lunch is at 13:00."),
    ]
    .into_iter()
    .enumerate()
    {
        updates.push(Update::Chat {
            update_id: 1 + 2 * at as i64,
            chat: info(id, ChatKind::Session {}, title),
        });
        updates.push(Update::Message {
            update_id: 2 + 2 * at as i64,
            chat_id: id,
            message: reply(10 + at as i64, text),
        });
    }
    chats.apply(page(4, updates));
    assert_eq!(chats.sessions().unwrap().len(), 2);

    assert!(chats.hide_alerts(HashSet::from([alert])));
    let titles: Vec<String> = chats
        .sessions()
        .unwrap()
        .into_iter()
        .map(|s| s.title)
        .collect();
    assert_eq!(titles, ["Lunch"]);
    assert_eq!(chats.search("hotel").unwrap().len(), 1);
    // The same set again changes nothing.
    assert!(!chats.hide_alerts(HashSet::from([alert])));

    // A reply makes it a chat: the server stops listing it.
    assert!(chats.hide_alerts(HashSet::new()));
    assert_eq!(chats.sessions().unwrap().len(), 2);
    assert_eq!(chats.search("hotel").unwrap().len(), 2);
    chats.clear();
}

#[test]
fn a_step_outside_the_turn_leaves_its_tools_alone() {
    let messages = [
        user(1, "Where is lunch?"),
        in_turn(1, tool(2, "open_maps", "{}")),
        in_turn(1, reply(3, "Here it is.")),
        reply(4, "Your routine ran."),
    ];
    let turns = turns(&messages);
    let [
        Turn::User { .. },
        Turn::Assistant { tools, .. },
        Turn::Assistant {
            html, tools: none, ..
        },
    ] = turns.as_slice()
    else {
        panic!("unexpected turns: {turns:?}");
    };
    assert_eq!(tools[0].name, "open_maps");
    assert!(html.contains("routine ran"));
    assert!(none.is_empty());
}

#[test]
fn the_copy_follows_the_log_and_survives_a_restart() {
    let dir = dir("follow");
    let (session, group) = (Uuid::new_v4(), Uuid::new_v4());
    let chats = Chats::load(dir.clone());
    assert!(chats.sessions().is_none());

    let changed = chats.apply(page(
        5,
        vec![
            Update::Chat {
                update_id: 1,
                chat: info(session, ChatKind::Session {}, "Lunch"),
            },
            Update::Message {
                update_id: 2,
                chat_id: session,
                message: user(10, "hi"),
            },
            Update::Message {
                update_id: 3,
                chat_id: session,
                message: reply(11, "hello"),
            },
            Update::Chat {
                update_id: 4,
                chat: ChatInfo {
                    members: vec![GroupMember {
                        name: "Ali".into(),
                        you: true,
                        creator: true,
                        joined_at: 0,
                    }],
                    ..info(group, ChatKind::Group {}, "Trip")
                },
            },
            Update::Message {
                update_id: 5,
                chat_id: group,
                message: message(
                    12,
                    MessageBody::Beep {
                        content: "**hey**".into(),
                    },
                ),
            },
        ],
    ));
    assert_eq!(changed.sessions, HashSet::from([session]));
    assert_eq!(changed.groups, HashSet::from([group]));
    let sessions = chats.sessions().unwrap();
    assert_eq!(sessions[0].title, "Lunch");
    assert_eq!(sessions[0].updated_at, 11_000);
    assert_eq!(chats.session(session).unwrap().len(), 2);
    let view = chats.group(group).unwrap();
    assert_eq!(view.messages[0].seq, 12);
    let listed = chats.groups().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "Trip");
    assert_eq!(listed[0].last.as_ref().map(|m| m.seq), Some(12));
    assert_eq!(listed[0].updated_at, 12_000);
    assert!(view.messages[0].html.contains("<strong>hey</strong>"));
    assert!(chats.session(group).is_none());

    chats.apply(page(
        7,
        vec![
            Update::Deleted {
                update_id: 6,
                chat_id: session,
                message_id: 11,
            },
            Update::ChatDeleted {
                update_id: 7,
                chat_id: group,
            },
        ],
    ));
    let reopened = Chats::load(dir.clone());
    assert_eq!(reopened.last(), 7);
    assert!(reopened.groups().unwrap().is_empty());
    assert_eq!(reopened.session(session).unwrap().len(), 1);
    assert!(reopened.group(group).is_none());
    assert!(!chat_file(&dir, group).exists());

    let changed = reopened.apply(UpdatePage {
        reset: true,
        ..page(1, Vec::new())
    });
    assert_eq!(changed.sessions, HashSet::from([session]));
    assert!(reopened.sessions().unwrap().is_empty());
    reopened.clear();
    assert!(!dir.exists());
}

#[test]
fn search_finds_the_latest_message_that_holds_the_words() {
    let dir = dir("search");
    let (lunch, trip) = (Uuid::new_v4(), Uuid::new_v4());
    let chats = Chats::load(dir.clone());
    assert!(chats.search("lunch").is_none());
    let mut updates = vec![
        Update::Chat {
            update_id: 1,
            chat: info(lunch, ChatKind::Session {}, "Lunch"),
        },
        Update::Chat {
            update_id: 2,
            chat: info(trip, ChatKind::Session {}, "Trip"),
        },
    ];
    let messages = [
        (lunch, user(10, "Where is Kimpton?")),
        (lunch, tool(11, "open_maps", "kimpton")),
        (lunch, reply(12, "The kimpton is on Main St.")),
        (trip, user(13, "Book the Kimpton")),
        (trip, tool(14, "book", "kimpton")),
    ];

    for (at, (chat_id, message)) in messages.into_iter().enumerate() {
        updates.push(Update::Message {
            update_id: 3 + at as i64,
            chat_id,
            message,
        });
    }

    chats.apply(page(7, updates));

    let found = chats.search(" KIMPTON ").unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].session.title, "Trip");
    assert!(found[0].from_user);
    assert_eq!(found[1].snippet, "The kimpton is on Main St.");
    assert!(!found[1].from_user);
    assert!(chats.search("open_maps").unwrap().is_empty());
    assert!(chats.search("k").unwrap().is_empty());
    chats.clear();
}
