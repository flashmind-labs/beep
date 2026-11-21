use super::*;
use std::fs;

#[test]
fn a_conversation_folder_is_under_the_hidden_assistant_folder() {
    let home = Path::new("/home/user");
    let root = home.join(".beep").join("assistant");
    let made = folder(home, &SessionId::from("ab12cd"));

    assert_eq!(made, root.join("ab12cd"));
    assert!(holds(home, &made));
    assert!(!holds(home, home));
    assert!(!holds(home, &root));
    assert!(!holds(home, &root.join("a").join("b")));
    assert!(!holds(home, &home.join("projects").join("x")));
}

#[test]
fn only_a_conversation_folder_is_removed() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let home = dir.path();
    let conversation = folder(home, &SessionId::from("ab12cd"));
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

#[test]
fn a_new_conversation_that_asks_for_no_folder_gets_its_own() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let home = dir.path();
    let id = SessionId::from("ab12cd");

    for asked in [None, Some(""), Some(" ~ ")] {
        let made = own(home, asked, &id).expect("the folder is made");
        assert_eq!(made, Some(folder(home, &id)), "{asked:?}");
        assert!(folder(home, &id).is_dir());
    }

    // One that names a folder runs there.
    let other = SessionId::from("ef34ab");
    assert_eq!(own(home, Some("/srv/project"), &other).expect("no folder to make"), None);
    assert_eq!(own(home, Some("~/notes"), &other).expect("no folder to make"), None);
    assert!(!folder(home, &other).exists());
}
