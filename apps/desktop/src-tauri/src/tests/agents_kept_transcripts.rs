use super::*;
use serde_json::json;
use std::time::Duration;

fn store() -> Store {
    Store::new(std::env::temp_dir().join(format!("beep-kept-{}", uuid::Uuid::new_v4())))
}

fn key(session: &str) -> SessionKey {
    SessionKey {
        machine_id: "build-box".into(),
        agent: "claude".into(),
        session: session.into(),
    }
}

fn page(text: &str) -> KeptTranscript {
    KeptTranscript {
        page: json!({"entries": [{"kind": "user", "text": text}], "earlier": 0}),
        cursor: Some(SessionCursor {
            start: 0,
            next: 1,
            fingerprint: "f".into(),
            prefix_next: 0,
            prefix_fingerprint: String::new(),
        }),
    }
}

#[test]
fn a_kept_page_comes_back_and_goes_with_its_session() {
    let mut kept = store();
    assert!(kept.read(&key("s1")).is_none());

    kept.write(&key("s1"), &page("hello")).unwrap();
    kept.write(&key("s1"), &page("again")).unwrap();
    let back = kept.read(&key("s1")).expect("the page is kept");
    assert_eq!(back.page["entries"][0]["text"], "again");
    assert_eq!(back.cursor.map(|cursor| cursor.next), Some(1));
    assert!(kept.read(&key("s2")).is_none());

    kept.forget(&key("s1"));
    assert!(kept.read(&key("s1")).is_none());
    // A follow ending after the delete does not write it back.
    kept.write(&key("s1"), &page("late")).unwrap();
    assert!(kept.read(&key("s1")).is_none());
    let _ = fs::remove_dir_all(&kept.dir);
}

#[test]
fn a_page_over_the_limit_is_refused() {
    let kept = store();
    let big = KeptTranscript {
        page: json!("x".repeat(MAX_ONE)),
        cursor: None,
    };
    assert!(kept.write(&key("s1"), &big).is_err());
    assert!(kept.read(&key("s1")).is_none());
}

#[test]
fn trimming_keeps_the_newest_within_both_limits() {
    let kept = store();

    for session in ["s1", "s2", "s3"] {
        kept.write(&key(session), &page(session)).unwrap();
        // Distinct times, so the newest is plain.
        std::thread::sleep(Duration::from_millis(20));
    }

    trim(&kept.dir, 2, u64::MAX);
    assert!(kept.read(&key("s1")).is_none());
    assert!(kept.read(&key("s3")).is_some());

    let one = fs::metadata(kept.file(&key("s3"))).unwrap().len();
    trim(&kept.dir, 10, one);
    assert!(kept.read(&key("s2")).is_none());
    assert!(kept.read(&key("s3")).is_some());
    let _ = fs::remove_dir_all(&kept.dir);
}

#[test]
fn an_unreadable_file_is_dropped() {
    let kept = store();
    kept.write(&key("s1"), &page("hello")).unwrap();
    fs::write(kept.file(&key("s1")), b"{").unwrap();
    assert!(kept.read(&key("s1")).is_none());
    assert!(!kept.file(&key("s1")).exists());
    let _ = fs::remove_dir_all(&kept.dir);
}

#[tokio::test]
async fn the_thread_keeps_and_reads_in_order() {
    let dir = std::env::temp_dir().join(format!("beep-kept-{}", uuid::Uuid::new_v4()));
    let kept = KeptTranscripts::new(dir.clone());
    kept.keep(key("s1"), page("first"));
    kept.keep(key("s1"), page("second"));
    let back = kept
        .read(key("s1"))
        .await
        .unwrap()
        .expect("the page is kept");
    assert_eq!(back.page["entries"][0]["text"], "second");
    let _ = fs::remove_dir_all(dir);
}
