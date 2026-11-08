use super::*;

fn notice(title: &str, body: &str) -> Notice {
    Notice {
        title: title.into(),
        body: body.into(),
        session_id: String::new(),
        app: None,
        approval: None,
        changed: false,
        group: false,
    }
}

#[test]
fn the_card_shows_the_title_and_the_start_of_the_text() {
    let card = NoticeCard::of(1, &notice(" Inbox sweep ", "Two emails\n\nneed a reply."));
    assert_eq!(card.title, "Inbox sweep");
    assert_eq!(card.snippet, "Two emails need a reply.");
    let long = NoticeCard::of(2, &notice("", &"word ".repeat(100)));
    assert_eq!(long.title, "Beep");
    assert!(long.snippet.ends_with('…'));
    assert!(long.snippet.chars().count() <= SNIPPET + 1);
}
