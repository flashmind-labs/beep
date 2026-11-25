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

#[test]
fn a_folder_no_window_follows_is_forgotten_and_the_places_of_a_machine_without_folders_with_it() {
    let (machine, _) = key("/srv/one");
    let other = iroh::SecretKey::from_bytes(&[8; 32]).public();
    let mut held = Held::default();
    held.folders
        .insert((machine, "/srv/one".into()), Folder::default());
    held.folders
        .insert((machine, "/srv/two".into()), Folder::default());
    held.folders
        .insert((other, "/srv/one".into()), Folder::default());
    held.places.insert(machine, Vec::new());
    held.places.insert(other, Vec::new());

    held.keep(|on, root| on == machine && root == "/srv/two");

    assert_eq!(held.folders.len(), 1);
    assert!(held.folders.contains_key(&(machine, "/srv/two".to_owned())));
    assert!(held.places.contains_key(&machine));
    assert!(!held.places.contains_key(&other));
}

#[test]
fn the_reads_of_a_closed_window_go_and_a_read_two_windows_asked_stays_until_both_go() {
    let (machine, _) = key("/srv/one");
    let mut held = Held::default();
    let folder = held
        .folders
        .entry((machine, "/srv/one".into()))
        .or_default();
    let read = |path: &str| TreeRead {
        path: path.into(),
        ignored: false,
        deep: false,
    };
    keep(&mut folder.trees, ("one".into(), read("src")), KEPT);
    keep(&mut folder.trees, ("two".into(), read("src")), KEPT);
    keep(&mut folder.trees, ("two".into(), read("docs")), KEPT);
    keep(
        &mut held.places.entry(machine).or_default(),
        ("two".into(), Vec::new()),
        PLACES_KEPT,
    );

    held.forget_windows(&HashSet::from(["one".to_owned()]));
    assert!(held.folders.is_empty() && held.places.is_empty());

    let folder = held
        .folders
        .entry((machine, "/srv/one".into()))
        .or_default();
    keep(&mut folder.trees, ("one".into(), read("src")), KEPT);
    keep(&mut folder.trees, ("two".into(), read("src")), KEPT);
    keep(&mut folder.trees, ("two".into(), read("docs")), KEPT);
    held.forget_windows(&HashSet::from(["two".to_owned()]));

    let kept = &held.folders[&(machine, "/srv/one".to_owned())];
    assert_eq!(distinct(&kept.trees).len(), 2);
    held.forget_windows(&HashSet::new());
    assert!(held.folders.is_empty());
}

#[test]
fn one_list_of_a_folder_says_which_worktrees_each_session_created() {
    let tree = |path: &str, by: &[&str]| Worktree {
        path: path.into(),
        branch: None,
        head: "abc".into(),
        own: false,
        created: false,
        created_by: by.iter().map(|id| SessionId::from(*id)).collect(),
    };
    let list = [tree("/srv/main", &[]), tree("/srv/fix", &["a", "b"])];

    let for_a = created_by(&list, Some(&SessionId::from("a")));
    assert_eq!(
        for_a.iter().map(|t| t.created).collect::<Vec<_>>(),
        [false, true]
    );
    let for_c = created_by(&list, Some(&SessionId::from("c")));
    assert!(for_c.iter().all(|t| !t.created));
    assert!(created_by(&list, None).iter().all(|t| !t.created));
}
