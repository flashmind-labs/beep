use super::*;

fn png(fill: u8, size: usize) -> Picture {
    let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
    data.resize(size, fill);
    Picture {
        mime: "image/png".into(),
        data,
    }
}

#[test]
fn a_session_keeps_only_its_newest_pictures() {
    let images = Images::default();
    let first = png(0, 64);
    images.keep("a1", first.clone());

    for fill in 1..=u8::try_from(SESSION_IMAGES).unwrap() {
        images.keep("a1", png(fill, 64));
    }

    assert_eq!(images.get("a1", &image_key(&first.data)), None);
    let newest = png(u8::try_from(SESSION_IMAGES).unwrap(), 64);
    assert!(images.get("a1", &image_key(&newest.data)).is_some());
    // Another session's are left alone.
    let other = png(0, 65);
    images.keep("b2", other.clone());
    images.keep("a1", png(200, 64));
    assert!(images.get("b2", &image_key(&other.data)).is_some());
}

#[test]
fn all_sessions_together_stay_under_the_overall_limit() {
    let images = Images::default();
    let first = png(0, 64);
    images.keep("s0", first.clone());

    for n in 1..=MAX_IMAGES {
        images.keep(&format!("s{n}"), png(1, 64 + n));
    }

    assert_eq!(images.get("s0", &image_key(&first.data)), None);
    assert_eq!(lock(&images.kept).held.len(), MAX_IMAGES);
    let big = png(2, SESSION_BYTES + 1);
    images.keep("s0", big.clone());
    assert_eq!(images.get("s0", &image_key(&big.data)), None, "too big");
}

#[test]
fn new_pictures_wait_for_the_server_a_bounded_few() {
    use futures::FutureExt;
    let images = Images::default();
    images.keep_and_send("a1", png(0, 64));
    images.keep_and_send("a1", png(0, 64));
    images.keep("a1", png(1, 64));
    let first = images.next_unsent().now_or_never().expect("waiting");
    assert_eq!(first.picture.data, png(0, 64).data);
    assert!(
        images.next_unsent().now_or_never().is_none(),
        "a picture the session had, or one only kept, is not sent"
    );

    for n in 0..MAX_UNSENT + 2 {
        images.keep_and_send(&format!("s{n}"), png(2, 64 + n));
    }

    let oldest = images.next_unsent().now_or_never().expect("waiting");
    assert_eq!(oldest.session, "s2", "the oldest went");
}

#[test]
fn a_picture_only_a_subagent_wrote_is_found_from_its_parent() {
    use base64::{Engine, engine::general_purpose::STANDARD};

    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("p1.jsonl");
    std::fs::write(&parent, "{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n").unwrap();
    let subagents = dir.path().join("p1/subagents");
    std::fs::create_dir_all(&subagents).unwrap();
    let shot = png(7, 64);
    let line = serde_json::json!({"type": "user", "message": {"content": [
        {"type": "tool_result", "content": [
            {"type": "image", "source": {"type": "base64", "media_type": "image/png",
                "data": STANDARD.encode(&shot.data)}}
        ]}
    ]}});
    std::fs::write(subagents.join("agent-a1.jsonl"), format!("{line}\n")).unwrap();

    let images = Images::default();
    let found = images.find_in_file(&parent, &image_key(&shot.data)).unwrap();
    assert_eq!(found.map(|p| p.data), Some(shot.data));
    assert_eq!(images.find_in_file(&parent, &image_key(&png(8, 64).data)).unwrap(), None);
}
