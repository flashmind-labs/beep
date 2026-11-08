use super::*;

fn mark(id: &str, at_ms: u64) -> Mark {
    Mark {
        id: id.into(),
        at_ms,
    }
}

fn listed(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

#[test]
fn a_new_read_replaces_the_old_mark_of_its_id() {
    let held = vec![mark("a", 1), mark("b", 2)];
    let marks = merge(held, &["a".into()], &listed(&["a", "b"]), 9);
    assert_eq!(marks, [mark("a", 9), mark("b", 2)]);
}

#[test]
fn marks_of_notifications_no_longer_listed_are_dropped() {
    let held = vec![mark("gone", 5), mark("b", 2)];
    let marks = merge(held, &["a".into()], &listed(&["a", "b"]), 9);
    assert_eq!(marks, [mark("a", 9), mark("b", 2)]);
}

#[test]
fn only_the_newest_marks_are_kept() {
    let ids: Vec<String> = (0..KEEP + 20).map(|i| format!("n{i}")).collect();
    let all: HashSet<String> = ids.iter().cloned().collect();
    let held: Vec<Mark> = ids
        .iter()
        .zip(1u64..)
        .map(|(id, at)| mark(id, at))
        .collect();
    let marks = merge(held, &[], &all, 0);
    assert_eq!(marks.len(), KEEP);
    assert_eq!(marks[0].id, format!("n{}", KEEP + 19));
}

#[test]
fn marks_are_positional_msgpack() {
    let bytes = Marks {
        marks: vec![mark("a", 1)],
    }
    .to_bytes()
    .expect("encodes");
    // [[["a", 1]]]
    assert_eq!(bytes, [0x91, 0x91, 0x92, 0xa1, b'a', 0x01]);
    let back = Marks::from_bytes(&bytes).expect("reads");
    assert_eq!(back.marks, [mark("a", 1)]);
}
