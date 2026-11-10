use super::*;
use serde_json::json;

fn cursor(start: u64, next: u64) -> SessionCursor {
    SessionCursor {
        start,
        next,
        fingerprint: next.to_string(),
    }
}

fn page(earlier: u64, texts: &[&str]) -> Value {
    let entries: Vec<Value> = texts
        .iter()
        .map(|text| json!({"kind": "assistant", "text": text}))
        .collect();
    json!({"earlier": earlier, "entries": entries, "turns": earlier})
}

fn texts(held: &Held) -> Vec<&str> {
    held.page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["text"].as_str().unwrap())
        .collect()
}

#[test]
fn updates_join_as_the_webview_joins_them() {
    let held = join(None, false, page(0, &["a", "b"]), &cursor(0, 2)).expect("a snapshot");

    // A delta that rewrites the last entry and adds one; the newest fields win.
    let held = join(Some(held), true, page(1, &["B", "c"]), &cursor(0, 3)).expect("a delta");
    assert_eq!(texts(&held), ["a", "B", "c"]);
    assert_eq!(held.page["turns"], 1);
    assert_eq!(held.cursor.next, 3);

    // The node moves its window on: the page starts at the cursor.
    let held = join(Some(held), true, page(3, &["d"]), &cursor(2, 4)).expect("a delta");
    assert_eq!(texts(&held), ["c", "d"]);
    assert_eq!(held.page["earlier"], 2);

    // A gap, or a delta with nothing to join, is dropped.
    assert!(join(Some(held), true, page(6, &["g"]), &cursor(2, 7)).is_none());
    assert!(join(None, true, page(0, &["a"]), &cursor(0, 1)).is_none());
    // A page that does not end at its cursor is dropped.
    assert!(join(None, false, page(0, &["a"]), &cursor(0, 2)).is_none());
}
