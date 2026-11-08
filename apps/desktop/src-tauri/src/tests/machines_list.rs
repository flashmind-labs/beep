use super::*;

#[test]
fn a_machine_only_the_server_lists_is_added_to_the_mesh() {
    let mesh = vec!["studio".to_owned(), "box".to_owned()];
    let server = ["box".to_owned(), "phone-host".to_owned()];
    let merged = outside(mesh, &server, |id| id);
    assert_eq!(merged, ["studio", "box", "phone-host"]);
}

#[test]
fn a_server_that_cannot_answer_leaves_the_last_list() {
    let held = Arc::new(Held::<String>::default());
    let refresh = held.begin().expect("the first refresh starts");
    assert!(held.begin().is_none(), "one refresh runs at a time");
    assert!(held.finish(Ok(vec!["far".to_owned()]), |a, b| a == b));
    drop(refresh);
    let refresh = held.begin().expect("the next refresh starts");
    assert!(!held.finish(Err(DesktopError::NoServer), |a, b| a == b));
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
fn with_no_server_the_mesh_machines_stand_alone() {
    let held = Held::<String>::default();
    assert_eq!(
        outside(vec!["studio".to_owned()], &held.last(), |id| id),
        ["studio"]
    );
}

#[test]
fn a_machine_the_server_does_not_list_counts_as_dropped() {
    assert!(dropped(&Ok(())));
    assert!(dropped(&Err(DesktopError::Api(ApiError::NotFound {}))));
    assert!(dropped(&Err(DesktopError::Api(ApiError::bad_request(
        "unknown request"
    )))));
    assert!(!dropped(&Err(DesktopError::BadRequest(
        "took too long".into()
    ))));
    assert!(!dropped(&Err(DesktopError::Api(ApiError::Internal {}))));
    assert!(!dropped(&Err(DesktopError::NoServer)));
}
