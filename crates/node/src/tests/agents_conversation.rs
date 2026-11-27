use super::*;
use std::fs;

/// The folder of the conversation `id` under `home`.
fn folder(home: &Path, id: &str) -> PathBuf {
    home.join(".beep").join("assistant").join(id)
}

#[test]
fn a_conversation_folder_is_under_the_hidden_assistant_folder() {
    let home = Path::new("/home/user");
    let root = home.join(".beep").join("assistant");
    let made = folder(home, "ab12cd");

    assert_eq!(made, root.join("ab12cd"));
    assert!(holds(home, &made));
    assert!(!holds(home, home));
    assert!(!holds(home, &root));
    assert!(!holds(home, &root.join("a").join("b")));
    assert!(!holds(home, &root.join("..")));
    assert!(!holds(home, &root.join(".")));
    assert!(!holds(home, &home.join("projects").join("x")));
}

#[test]
fn only_a_conversation_folder_is_removed() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let home = dir.path();
    let conversation = folder(home, "ab12cd");
    let kept = home.join("keep");
    fs::create_dir_all(&conversation).expect("the conversation's folder");
    fs::create_dir_all(&kept).expect("another folder");
    fs::write(conversation.join("notes.md"), "found").expect("a note");
    fs::write(kept.join("file"), "mine").expect("a file");

    remove(home, home).expect("the home folder is left alone");
    remove(home, &kept).expect("another folder is left alone");
    assert!(conversation.join("notes.md").exists());
    assert!(kept.join("file").exists());

    remove(home, &conversation).expect("the conversation's folder goes");
    assert!(!conversation.exists());
    assert!(home.join(".beep").join("assistant").exists());
    assert!(kept.join("file").exists());
    remove(home, &conversation).expect("one that is gone is no failure");
}

/// Ids to hand out in turn.
fn ids<'a>(given: &'a [&'a str]) -> impl FnMut() -> SessionId + 'a {
    let mut given = given.iter();
    move || SessionId::from(*given.next().expect("another id"))
}

#[test]
fn a_new_conversation_that_asks_for_no_folder_gets_its_own() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let home = dir.path();

    for (asked, id) in [
        (None, "ab12c0"),
        (Some(""), "ab12c1"),
        (Some(" ~ "), "ab12c2"),
    ] {
        let made = own(home, asked, ids(&[id])).expect("the folder is made");
        assert_eq!(made, Some(folder(home, id)), "{asked:?}");
        assert!(folder(home, id).is_dir());
    }

    // One that names a folder runs there.
    assert_eq!(
        own(home, Some("/srv/project"), ids(&["ef34ab"])).expect("no folder to make"),
        None
    );
    assert_eq!(
        own(home, Some("~/notes"), ids(&["ef34ab"])).expect("no folder to make"),
        None
    );
    assert!(!folder(home, "ef34ab").exists());
}

#[test]
fn a_new_conversation_never_takes_a_folder_that_is_there() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let home = dir.path();
    let first = own(home, None, ids(&["ab12cd"]))
        .expect("the folder is made")
        .expect("its own");
    fs::write(first.join("notes.md"), "found").expect("a note");

    let second = own(home, None, ids(&["ab12cd", "ab12cd", "ef34ab"]))
        .expect("the folder is made")
        .expect("its own");
    assert_eq!(second, folder(home, "ef34ab"));
    assert_eq!(
        fs::read_to_string(first.join("notes.md")).expect("the note"),
        "found"
    );

    // Ids that are all taken make no conversation.
    let taken = ["ab12cd"; 16];
    assert!(own(home, None, ids(&taken)).is_err());
}

#[test]
fn a_thread_of_a_conversation_gets_a_subfolder_named_after_it() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let home = dir.path();
    let main = own(home, None, ids(&["ab12cd"]))
        .expect("the folder is made")
        .expect("a folder of its own");
    let thread = |parent: &Path, title: Option<&str>, id: &str| {
        thread_folder(home, parent, title, ids(&[id])).expect("the folder is made")
    };

    let first = thread(&main, Some("Solid-state timelines"), "111111").expect("a subfolder");
    assert_eq!(first, main.join("solid-state-timelines"));
    assert!(first.is_dir());
    // A second thread of that name, and one with no name, still get their own.
    let twin = thread(&main, Some("Solid-state timelines"), "222222");
    assert_eq!(twin, Some(main.join("solid-state-timelines-222222")));
    assert_eq!(thread(&main, None, "333333"), Some(main.join("333333")));
    assert_eq!(
        thread(&main, Some(" / "), "444444"),
        Some(main.join("444444"))
    );
    // A thread's own thread goes under it.
    assert_eq!(
        thread(&first, Some("EU only"), "555555"),
        Some(first.join("eu-only"))
    );
    // A suffix that is taken too is passed over.
    let third = thread_folder(
        home,
        &main,
        Some("Solid-state timelines"),
        ids(&["222222", "777777"]),
    );
    assert_eq!(
        third.expect("the folder is made"),
        Some(main.join("solid-state-timelines-777777"))
    );

    // A chat anywhere else keeps its children in its own folder.
    let project = home.join("projects").join("x");
    assert_eq!(thread(&project, Some("Fix it"), "666666"), None);
    assert_eq!(thread(home, Some("Fix it"), "666666"), None);
    assert!(!project.exists());
}

#[test]
fn threads_started_together_under_one_name_get_a_folder_each() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let home = dir.path();
    let main = own(home, None, ids(&["ab12cd"]))
        .expect("the folder is made")
        .expect("its own");
    let gate = std::sync::Barrier::new(16);

    let made: std::collections::HashSet<PathBuf> = std::thread::scope(|scope| {
        let starts: Vec<_> = (0..16)
            .map(|at| {
                let (gate, main) = (&gate, &main);
                scope.spawn(move || {
                    gate.wait();
                    let id = move || SessionId::from(format!("{at:06x}"));
                    thread_folder(home, main, Some("Same name"), id).expect("the folder is made")
                })
            })
            .collect();
        starts
            .into_iter()
            .filter_map(|start| start.join().expect("a start"))
            .collect()
    });

    assert_eq!(made.len(), 16, "{made:?}");
}

#[test]
fn only_an_assistant_in_a_folder_of_its_own_is_told_to_keep_notes_there() {
    let home = Path::new("/home/user");

    assert_eq!(
        brief(home, &folder(home, "ab12cd")),
        Some(crate::agents::brief::OWN_FOLDER)
    );
    // One from before these folders runs in the home folder, and one may
    // name any folder: neither is told the folder is its own.
    assert_eq!(brief(home, home), None);
    assert_eq!(brief(home, &home.join("projects").join("x")), None);
    assert_eq!(brief(home, &folder(home, "ab12cd").join("a-thread")), None);
    assert!(!crate::agents::brief::ASSISTANT.contains("notes.md"));
}

#[test]
fn a_moving_session_keeps_its_place_under_the_chats_folder() {
    let home = Path::new("/srv/home");
    assert_eq!(
        moved(home, "ab12/research"),
        Some(home.join(".beep/assistant/ab12/research"))
    );
    assert_eq!(moved(home, "ab12"), Some(home.join(".beep/assistant/ab12")));

    for outside in ["", "..", "ab12/..", "/etc", "ab12//x", "./ab12"] {
        assert_eq!(moved(home, outside), None, "{outside:?}");
    }
}
