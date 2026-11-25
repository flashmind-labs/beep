use super::*;

fn key(root: &str) -> (EndpointId, Topic) {
    let machine = EndpointId::from_bytes(&[7; 32]).expect("a public key");
    (machine, Topic::keyed("files", root))
}

#[test]
fn a_revision_that_comes_while_reading_is_read_once_more_and_only_the_last() {
    let mut held = Held::default();
    let at = key("/srv/project");

    assert!(held.begin(at.clone(), 1));
    assert!(!held.begin(at.clone(), 2));
    assert!(!held.begin(at.clone(), 3));

    assert_eq!(held.next(&at), Some(3));
    assert_eq!(held.next(&at), None);
    // The reading is over: the next revision starts one.
    assert!(held.begin(at, 4));
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
