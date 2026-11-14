use super::*;

fn id() -> PluginId {
    "store-test".parse().unwrap()
}

#[test]
fn a_set_over_the_cap_is_refused_and_a_delete_frees_room() {
    let folder = tempfile::tempdir().unwrap();
    let mut store = Store::load(id(), folder.path().join("store-test.json"));
    let now = Instant::now();
    let big = format!("\"{}\"", "x".repeat(MAX_BYTES / 2));

    store.set("a", &big, now).unwrap();
    assert!(matches!(store.set("b", &big, now), Err(StoreError::Full)));
    store.set("a", "1", now).unwrap();
    store.set("b", &big, now).unwrap();
    store.delete("b", now);
    store.set("c", &big, now).unwrap();

    assert_eq!(store.get("a").as_deref(), Some("1"));
    assert_eq!(store.get("b"), None);
}

#[test]
fn a_corrupt_file_reads_as_empty() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("store-test.json");
    std::fs::write(&path, "{ not json").unwrap();

    let store = Store::load(id(), path);

    assert_eq!(store.get("a"), None);
}
