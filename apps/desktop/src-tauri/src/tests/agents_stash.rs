use super::*;

fn message(text: &str) -> Message {
    Message {
        text: text.into(),
        ..Message::default()
    }
}

#[test]
fn stash_survives_reopening_with_attachment_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stash.sqlite");
    let db = open(&path).unwrap();
    let mut original = message("Review the attached file");
    original.files.push(Attachment {
        name: "sample.bin".into(),
        mime: "application/octet-stream".into(),
        size: 3,
        data: "AAH/".into(),
    });
    let id = put(&db, "project-a", &original).unwrap();
    drop(db);
    let db = open(&path).unwrap();
    assert_eq!(read(&db, "project-a", &id).unwrap().files[0].data, "AAH/");
    assert!(read(&db, "project-b", &id).is_err());
}

#[test]
fn pop_and_swap_are_atomic_and_only_succeed_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = open(&dir.path().join("stash.sqlite")).unwrap();
    let id = put(&db, "project", &message("stashed")).unwrap();
    assert_eq!(
        take(&mut db, "project", &id, Some(&message("draft")))
            .unwrap()
            .text,
        "stashed"
    );
    assert!(take(&mut db, "project", &id, Some(&message("duplicate"))).is_err());
    let count: u32 = db
        .query_row("SELECT COUNT(*) FROM stash", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let text: String = db
        .query_row(
            "SELECT json_extract(body, '$.text') FROM stash",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(text, "draft");
}

#[test]
fn a_failed_swap_keeps_the_original_stash_entry() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = open(&dir.path().join("stash.sqlite")).unwrap();
    let id = put(&db, "project", &message("original")).unwrap();
    assert!(take(&mut db, "project", &id, Some(&Message::default())).is_err());
    assert_eq!(read(&db, "project", &id).unwrap().text, "original");
}
