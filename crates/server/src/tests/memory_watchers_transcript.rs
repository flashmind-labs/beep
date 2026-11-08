use super::*;
use crate::store::TurnRecord;
use uuid::Uuid;

fn message(id: i64, seconds: i64, body: StoredBody) -> StoredMessage {
    StoredMessage {
        id,
        chat_id: Uuid::nil(),
        turn_id: None,
        sent_at: DateTime::UNIX_EPOCH + TimeDelta::seconds(seconds),
        author_id: None,
        body,
        images: Vec::new(),
    }
}

fn user(id: i64, seconds: i64, content: &str) -> StoredMessage {
    message(
        id,
        seconds,
        StoredBody::User {
            content: content.into(),
            turn: TurnRecord::default(),
        },
    )
}

fn beep(id: i64, seconds: i64, content: &str) -> StoredMessage {
    message(
        id,
        seconds,
        StoredBody::Assistant {
            content: content.into(),
        },
    )
}

#[test]
fn stops_at_the_budget_but_always_reads_one() {
    let long = "x".repeat(100);
    let messages = [user(1, 0, &long), beep(2, 1, &long), user(3, 2, &long)];
    let read = transcript(&messages, &HashMap::new(), 250);
    assert_eq!((read.first_seq, read.last_seq), (1, 2));
    let one = transcript(&messages, &HashMap::new(), 10);
    assert_eq!((one.first_seq, one.last_seq), (1, 1));
    let none = transcript(&[], &HashMap::new(), 10);
    assert_eq!((none.first_seq, none.last_seq), (0, 0));
    assert!(!none.talk);
}
