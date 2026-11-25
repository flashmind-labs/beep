use super::*;

fn key(root: &str) -> (EndpointId, Topic) {
    let machine = iroh::SecretKey::from_bytes(&[7; 32]).public();
    (machine, Topic::keyed("files", root))
}

fn update(revision: u64) -> Update {
    Update {
        machine_id: "machine".to_owned(),
        revision,
        topic: Topic::keyed("files", "/srv/project"),
        query: None,
        initial: false,
        data: Some(json!({ "changed": revision })),
        seq: 0,
    }
}

#[test]
fn an_update_that_comes_while_reading_is_read_once_more_and_only_the_last() {
    let mut held = Held::default();
    let at = key("/srv/project");

    assert!(held.begin(at.clone(), update(1)));
    assert!(!held.begin(at.clone(), update(2)));
    assert!(!held.begin(at.clone(), update(3)));

    // The last update is kept whole: it goes out as it came when the read finds nothing.
    let next = held.next(&at).expect("the last update waits");
    assert_eq!(next.revision, 3);
    assert_eq!(next.data, Some(json!({ "changed": 3 })));
    assert!(held.next(&at).is_none());
    // The reading is over: the next update starts one.
    assert!(held.begin(at, update(4)));
}

#[test]
fn a_read_asked_again_is_kept_once_and_the_oldest_go_past_the_limit() {
    let mut reads = vec!["a", "b"];
    keep(&mut reads, "a", 3);
    assert_eq!(reads, ["b", "a"]);

    keep(&mut reads, "c", 3);
    keep(&mut reads, "d", 3);
    assert_eq!(reads, ["a", "c", "d"]);
}
