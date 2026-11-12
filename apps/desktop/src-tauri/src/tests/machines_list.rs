use super::*;

#[test]
fn a_list_that_cannot_be_read_keeps_the_last_one() {
    let held = Arc::new(Held::<String>::default());
    let refresh = held.begin().expect("the first refresh starts");
    assert!(held.begin().is_none(), "one refresh runs at a time");
    assert!(held.finish(Ok(vec!["far".to_owned()]), |a, b| a == b));
    drop(refresh);
    let refresh = held.begin().expect("the next refresh starts");
    assert!(!held.finish(Err(DesktopError::NotInMesh), |a, b| a == b));
    assert_eq!(held.last(), ["far"]);
    drop(refresh);
    // A list that did not change is not news.
    assert!(!held.finish(Ok(vec!["far".to_owned()]), |a, b| a == b));
}

#[test]
fn a_refresh_that_panicked_does_not_block_the_next() {
    let held = Arc::new(Held::<String>::default());
    let refresh = held.begin().expect("the first refresh starts");
    let died = std::thread::spawn(move || {
        let _refresh = refresh;
        panic!("the task died");
    })
    .join();
    assert!(died.is_err());
    assert!(held.begin().is_some());
}

#[test]
fn a_live_value_that_moved_is_not_a_change() {
    // Entries are (id, load): only the id is structure.
    let held = Held::<(String, u32)>::default();
    let same = |a: &(String, u32), b: &(String, u32)| a.0 == b.0;
    assert!(held.finish(Ok(vec![("box".to_owned(), 3)]), same));
    assert!(!held.finish(Ok(vec![("box".to_owned(), 9)]), same));
    assert_eq!(held.last(), [("box".to_owned(), 9)]);
    // A machine that came or went is.
    assert!(held.finish(Ok(vec![("box".to_owned(), 9), ("rig".to_owned(), 1)]), same));
    assert!(held.finish(Ok(vec![("rig".to_owned(), 1)]), same));
}

#[test]
fn one_machines_change_replaces_only_its_rows() {
    let held = Held::<(String, u32)>::default();
    held.keep(vec![
        ("laptop".into(), 1),
        ("studio".into(), 1),
        ("studio".into(), 2),
    ]);
    assert!(held.was_read());

    held.replace(vec![("studio".into(), 3)], |(machine, _)| {
        machine == "studio"
    });
    assert_eq!(held.last(), [("laptop".into(), 1), ("studio".into(), 3)]);

    held.stale();
    assert!(!held.was_read());
}
