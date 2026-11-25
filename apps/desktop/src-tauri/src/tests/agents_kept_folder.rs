use super::*;

#[test]
fn images_forgotten_with_a_change_go_after_the_records_that_drop_them() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let (writes, queue) = mpsc::channel();
    // The folder doubles as the image folder, and the forgotten "file" is the
    // records' file itself: only the order of the two writes tells in the end.
    writes
        .send(Write::Forget(vec!["records".into()]))
        .expect("queued");
    writes.send(Write::Records(b"[]".to_vec())).expect("queued");
    drop(writes);

    write_all(dir.path(), dir.path(), "records", &queue);

    assert!(!dir.path().join("records").exists());
}
