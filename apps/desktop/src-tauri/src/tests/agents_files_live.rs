use super::*;
use crate::fleet::view_signal::changed_paths;

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
    let at = (machine, "/srv/one".to_owned());
    let mut held = Held::default();
    let read = |path: &str| TreeRead {
        path: path.into(),
        ignored: false,
        deep: false,
    };
    let folder = held.folders.entry(at.clone()).or_default();
    keep(&mut folder.trees, ("one".into(), read("src")), KEPT);
    keep(&mut folder.trees, ("two".into(), read("src")), KEPT);
    keep(&mut folder.trees, ("two".into(), read("docs")), KEPT);
    keep(
        held.places.entry(machine).or_default(),
        ("two".into(), Vec::new()),
        PLACES_KEPT,
    );

    // Window two closes: the read it shared with window one stays, its own goes.
    held.forget_windows(&HashSet::from(["one".to_owned()]));
    assert_eq!(distinct(&held.folders[&at].trees), [read("src")]);
    assert!(held.places.is_empty());

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

#[test]
fn the_update_that_waits_names_the_paths_of_those_it_replaced() {
    let listing = |revision, paths: &[&str]| Update {
        data: Some(json!({ "Sessions": { "Tree": { "tree": {}, "changed": paths } } })),
        ..update(revision)
    };
    let mut held = Held::default();
    let at = key("/srv/project");

    assert!(held.begin(at.clone(), listing(1, &["a.txt"])));
    assert!(!held.begin(at.clone(), listing(2, &["b.txt"])));
    assert!(!held.begin(at.clone(), listing(3, &["a.txt", "c.txt"])));
    assert_eq!(
        changed_paths(&held.next(&at).expect("the last update waits")),
        ["a.txt", "b.txt", "c.txt"]
    );

    // An update that does not say what changed leaves the whole unsaid.
    assert!(held.next(&at).is_none());
    assert!(held.begin(at.clone(), listing(4, &["a.txt"])));
    assert!(!held.begin(at.clone(), listing(5, &[])));
    assert!(!held.begin(at.clone(), listing(6, &["b.txt"])));
    assert!(changed_paths(&held.next(&at).expect("the last update waits")).is_empty());
}

#[test]
fn a_released_node_is_not_taken_to_name_who_created_a_worktree() {
    assert!(!names_creators(Some("0.8.27")));
    assert!(!names_creators(None));
    assert!(names_creators(Some("0.8.28")));
}

#[test]
fn a_change_reads_again_only_the_places_of_sessions_working_in_its_folder() {
    let (machine, _) = key("/srv/one");
    let ask = |session: &str| -> PlaceAsk {
        serde_json::from_value(json!({ "agent": "codex", "session": session, "external": true }))
            .expect("a place ask")
    };
    let here = vec![ask("here")];
    let elsewhere = vec![ask("elsewhere")];
    let unknown = vec![ask("unknown")];
    let mut held = Held::default();
    held.places.insert(
        machine,
        vec![
            ("main".into(), here.clone()),
            ("main".into(), elsewhere.clone()),
            ("main".into(), unknown),
        ],
    );
    held.placed
        .insert((machine, "here".into()), "/srv/one/.worktrees/fix".into());
    held.placed
        .insert((machine, "elsewhere".into()), "/srv/two".into());

    assert_eq!(held.places_in(machine, "/srv/one"), std::slice::from_ref(&here));
    assert_eq!(
        held.places_in(machine, "/srv/one/.worktrees/fix/src"),
        [here]
    );
    assert_eq!(held.places_in(machine, "/srv/two"), [elsewhere]);
    assert!(held.places_in(machine, "/srv/three").is_empty());

    // A session no set holds any longer forgets its checkout.
    held.places
        .get_mut(&machine)
        .expect("the machine's sets")
        .remove(0);
    held.forget_placed();
    assert!(!held.placed.contains_key(&(machine, "here".into())));
}
