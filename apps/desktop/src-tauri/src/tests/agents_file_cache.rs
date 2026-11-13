use super::*;

fn index(files: &[(&str, u64, u64)]) -> Index {
    Index {
        files: files
            .iter()
            .map(|&(hash, size, opened)| {
                let entry = Entry {
                    size,
                    opened,
                    sessions: BTreeSet::new(),
                };
                (hash.to_owned(), entry)
            })
            .collect(),
    }
}

fn sorted(mut gone: Vec<String>) -> Vec<String> {
    gone.sort();
    gone
}

#[test]
fn files_not_opened_for_keep_go() {
    let now = KEEP.as_secs() * 10;
    let index = index(&[("old", 1, now - KEEP.as_secs() - 1), ("new", 1, now - 5)]);

    assert_eq!(stale(&index, now, None), vec!["old".to_owned()]);
}

#[test]
fn the_oldest_go_beyond_the_cap_but_not_the_one_kept() {
    let now = 1_000;
    let half = MAX_BYTES / 2 + 1;
    let index = index(&[
        ("a", half, now - 3),
        ("b", half, now - 2),
        ("c", half, now - 1),
    ]);

    // `c` and `b` together are over the cap: `b` goes with `a`.
    assert_eq!(sorted(stale(&index, now, None)), vec!["a", "b"]);
    // `a` stays whatever its age, and with it no other one fits.
    assert_eq!(sorted(stale(&index, now, Some("a"))), vec!["b", "c"]);
}
