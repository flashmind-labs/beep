use super::*;

#[test]
fn a_read_walks_back_from_the_newest_page_to_what_was_indexed() {
    let indexed = Progress {
        done: 100,
        walk: None,
    };
    let newest = Page {
        earlier: 400,
        entries: 200,
    };
    let walking = advance(indexed, newest);
    assert_eq!(
        walking,
        Progress {
            done: 100,
            walk: Some(Walk { top: 600, low: 400 }),
        }
    );

    let older = Page {
        earlier: 200,
        entries: 200,
    };
    let walking = advance(walking, older);
    assert_eq!(walking.walk, Some(Walk { top: 600, low: 200 }));

    let reaching = Page {
        earlier: 0,
        entries: 200,
    };
    assert_eq!(
        advance(walking, reaching),
        Progress {
            done: 600,
            walk: None
        }
    );
}

#[test]
fn a_newest_page_that_reaches_what_was_indexed_needs_no_walk() {
    let indexed = Progress {
        done: 500,
        walk: None,
    };
    let newest = Page {
        earlier: 450,
        entries: 200,
    };
    assert_eq!(
        advance(indexed, newest),
        Progress {
            done: 650,
            walk: None
        }
    );
}

#[test]
fn stores_that_ignore_before_and_dropped_entries_end_a_walk() {
    let walking = Progress {
        done: 100,
        walk: Some(Walk { top: 600, low: 400 }),
    };
    let finished = Progress {
        done: 600,
        walk: None,
    };
    // A store that cannot read before an entry answers with its newest page.
    let newest = Page {
        earlier: 450,
        entries: 200,
    };
    assert_eq!(advance(walking, newest), finished);
    // Past dropped entries the answer is empty.
    let dropped = Page {
        earlier: 400,
        entries: 0,
    };
    assert_eq!(advance(walking, dropped), finished);
    assert!(!valid_page(
        400,
        Page {
            earlier: 250,
            entries: 200
        }
    ));
}

#[test]
fn a_transcript_numbered_again_is_read_again_from_the_start() {
    let indexed = Progress {
        done: 5000,
        walk: None,
    };
    // A large file read from a later point now has fewer entries.
    let newest = Page {
        earlier: 1800,
        entries: 200,
    };
    assert_eq!(
        advance(indexed, newest),
        Progress {
            done: 0,
            walk: Some(Walk {
                top: 2000,
                low: 1800
            }),
        }
    );
}
