use super::*;
use flashmind::core::ConversationEntry;

fn image(data: &str) -> ContentPart {
    ContentPart::Image {
        media_type: "image/jpeg".into(),
        data: data.into(),
    }
}

fn texts(parts: &[ContentPart]) -> Vec<String> {
    parts
        .iter()
        .map(|part| match part {
            ContentPart::Text { text } => text.clone(),
            ContentPart::Image { data, .. } => format!("<{data}>"),
            _ => String::new(),
        })
        .collect()
}

#[test]
fn pictures_keep_their_size_and_skip_what_is_not_an_image() {
    // A 1x1 PNG.
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";
    let kept = pictures(&[
        png.to_owned(),
        "data:image/png;base64,AAAA".to_owned(),
        "data:image/png;base64,not base64".to_owned(),
    ]);
    assert_eq!(kept.len(), 1);
    assert_eq!(
        (kept[0].mime.as_str(), kept[0].width, kept[0].height),
        ("image/png", 1, 1)
    );
}

#[test]
fn swaps_described_images_and_keeps_failed_ones() {
    let started = Utc::now();
    let mut conversation = Conversation::new();
    conversation.add(ConversationEntry::user_with_parts(
        "which is cheaper?",
        labelled(&[image("a"), image("b"), image("c")]),
    ));
    let descriptions = [Some("receipt, 4 EUR".into()), None, Some("menu".into())];

    replace_with_descriptions(&mut conversation, started, &descriptions);

    let EntryKind::User {
        parts: Some(parts), ..
    } = &conversation.entries()[0].kind
    else {
        panic!("the user message lost its parts");
    };
    assert_eq!(
        texts(parts),
        [
            "[Image 1: receipt, 4 EUR]",
            "Image 2:",
            "<b>",
            "[Image 3: menu]"
        ]
    );
}

#[test]
fn files_sent_with_images_stay() {
    let started = Utc::now();
    let mut conversation = Conversation::new();
    let file = ContentPart::Text {
        text: "<file name=\"a.csv\" type=\"text/csv\" size=\"3 bytes\">\na,b\n</file>".into(),
    };
    let mut parts = labelled(&[image("a")]);
    parts.push(file.clone());
    conversation.add(ConversationEntry::user_with_parts("what is this?", parts));

    replace_with_descriptions(&mut conversation, started, &[Some("a cat".into())]);

    let EntryKind::User {
        parts: Some(parts), ..
    } = &conversation.entries()[0].kind
    else {
        panic!("the user message lost its parts");
    };
    assert_eq!(
        texts(parts),
        ["[Image 1: a cat]".to_owned(), texts(&[file])[0].clone()]
    );
}
