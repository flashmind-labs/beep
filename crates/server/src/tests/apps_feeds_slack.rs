use super::*;

fn message(channel: &str, direct: bool, author: &str, n: i64, text: &str) -> Message {
    Message {
        id: format!("{channel}:{n}"),
        channel: channel.to_owned(),
        channel_name: channel.to_lowercase(),
        direct,
        at: DateTime::from_timestamp(1_790_000_000 + n * 60, 0).unwrap(),
        handle: format!("handle-{author}"),
        author_id: author.to_owned(),
        text: text.to_owned(),
    }
}

#[test]
fn parses_matches_and_drops_bots() {
    let channel = json!({"id": "D1", "name": "U2", "is_im": true, "is_mpim": false});
    let result = json!({"matches": [
        {"channel": channel, "user": "U2", "username": "sam", "ts": "1790000000.000100", "text": "hi"},
        {"channel": channel, "user": null, "username": "alerts", "ts": "1790000001.000000", "text": "up"},
    ]});
    let messages = parse_matches(&result);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].direct);
    assert_eq!(messages[0].id, "D1:1790000000.000100");
    assert_eq!(messages[0].handle, "sam");
}

#[test]
fn windows_surround_what_concerns_the_user() {
    // A busy channel: the user writes at 10 and is mentioned at 30.
    let mut messages: Vec<Message> = (0..40)
        .map(|n| message("C1", false, "U2", n, "chatter"))
        .collect();
    messages[10].author_id = "ME".to_owned();
    messages[30].text = "<@ME|me> send the numbers tomorrow".to_owned();
    // A channel the user only reads.
    messages.extend((0..5).map(|n| message("C2", false, "U3", n, "elsewhere")));
    // A direct message is always read.
    messages.push(message("D1", true, "U4", 50, "lunch?"));

    let windows = windows(messages.into_iter(), "ME");
    let spans: Vec<(&str, i64, i64)> = windows
        .iter()
        .map(|w| {
            let n = |m: &Message| (m.at.timestamp() - 1_790_000_000) / 60;
            (
                w.messages[0].channel.as_str(),
                n(&w.messages[0]),
                n(w.messages.last().unwrap()),
            )
        })
        .collect();
    assert_eq!(spans, [("C1", 7, 12), ("C1", 27, 32), ("D1", 50, 50)]);
}
