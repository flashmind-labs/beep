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
