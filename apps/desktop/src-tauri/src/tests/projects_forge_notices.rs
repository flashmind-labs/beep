use super::*;

fn item(host: &str, url: &str, review: bool, assigned: bool) -> Marked {
    Marked {
        host: host.into(),
        repo: "acme/widget".into(),
        number: 7,
        title: "Add a dark theme".into(),
        url: url.into(),
        updated_at: 1,
        review_requested: review,
        assigned,
    }
}

fn hosts(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn the_first_read_of_a_host_raises_nothing() {
    let mut seen = Seen::default();
    let items = [item("github.com", "u1", true, true)];
    assert_eq!(
        seen.update(&items, &hosts(&["github.com"])),
        Changes::default()
    );
    assert_eq!(
        seen.update(&items, &hosts(&["github.com"])),
        Changes::default()
    );
}

#[test]
fn a_flag_that_turns_on_is_raised_and_one_that_turns_off_is_cleared() {
    let mut seen = Seen::default();
    let answered = hosts(&["github.com"]);
    seen.update(&[item("github.com", "u1", false, false)], &answered);

    let asking = [item("github.com", "u1", true, false)];
    let changes = seen.update(&asking, &answered);
    assert_eq!(changes.raised, vec![(Flag::Review, &asking[0])]);
    assert!(changes.cleared.is_empty());

    let gone = seen.update(&[], &answered);
    assert!(gone.raised.is_empty());
    assert_eq!(gone.cleared, vec![(Flag::Review, "u1".to_owned())]);
}

#[test]
fn a_new_item_that_asks_is_raised_for_each_flag() {
    let mut seen = Seen::default();
    let answered = hosts(&["github.com"]);
    seen.update(&[], &answered);
    let items = [item("github.com", "u2", true, true)];
    let changes = seen.update(&items, &answered);
    assert_eq!(
        changes.raised,
        vec![(Flag::Review, &items[0]), (Flag::Assigned, &items[0])]
    );
}

#[test]
fn a_host_that_failed_keeps_its_flags_and_raises_nothing_when_back() {
    let mut seen = Seen::default();
    let both = hosts(&["github.com", "git.example.com"]);
    let items = [item("git.example.com", "u3", true, false)];
    seen.update(&items, &both);

    // The Forgejo host fails: its item is missing, but nothing ended.
    let failed = seen.update(&[], &hosts(&["github.com"]));
    assert_eq!(failed, Changes::default());

    // Back with the same item: not new.
    assert_eq!(seen.update(&items, &both), Changes::default());
}

#[test]
fn a_host_first_read_after_the_baseline_is_its_own_baseline() {
    let mut seen = Seen::default();
    seen.update(&[], &hosts(&["github.com"]));
    let late = [item("git.example.com", "u4", true, false)];
    assert_eq!(
        seen.update(&late, &hosts(&["github.com", "git.example.com"])),
        Changes::default()
    );
}
