use super::*;

#[test]
fn images_forgotten_with_a_change_go_after_the_records_that_drop_them() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let (writes, queue) = mpsc::channel();
    let (later, files) = mpsc::channel();
    // The folder doubles as the image folder, and the forgotten "file" is the
    // records' file itself: only the order of the two writes tells in the end.
    writes
        .send(Write::Forget(vec!["records".into()]))
        .expect("queued");
    writes.send(Write::Records(b"[]".to_vec())).expect("queued");
    drop(writes);

    write_all(dir.path(), dir.path(), "records", &queue, &later);
    drop(later);
    write_files(dir.path(), dir.path(), &files);

    assert!(!dir.path().join("records").exists());
}

#[test]
fn a_record_is_written_without_waiting_for_a_large_image_and_a_small_one_is_there_first() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let images = dir.path().join("images");
    let (writes, queue) = mpsc::channel();
    let (later, files) = mpsc::channel();
    let large: Arc<str> = "x".repeat(LARGE_IMAGE + 1).into();
    writes
        .send(Write::Image {
            file: "1-0".into(),
            data: large,
        })
        .expect("queued");
    writes
        .send(Write::Image {
            file: "2-0".into(),
            data: "small".into(),
        })
        .expect("queued");
    writes.send(Write::Records(b"[]".to_vec())).expect("queued");
    drop(writes);

    // Only the records' thread has run: the files' thread has not started on the large one.
    write_all(dir.path(), &images, "records", &queue, &later);

    assert!(dir.path().join("records").exists());
    assert!(images.join("2-0").exists());
    assert!(!images.join("1-0").exists());

    drop(later);
    write_files(dir.path(), &images, &files);

    assert!(images.join("1-0").exists());
}
