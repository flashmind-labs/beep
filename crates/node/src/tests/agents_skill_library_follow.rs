use super::*;
use std::fs;

/// A chat's picks go to its folder and to the folders its threads work in, not to other
/// folders in it, other chats' or a chat on another machine.
#[test]
fn a_chat_gets_its_skills_in_its_folder_and_its_threads() {
    let home = tempfile::tempdir().unwrap();
    let chats = home.path().join(".beep/assistant");
    let chat = chats.join("chat-one");
    let thread = chat.join("pricing");
    let nested = thread.join("deeper");
    let made = chat.join("17-events");
    let other = chats.join("chat-two/notes");
    for folder in [&nested, &made, &other] {
        fs::create_dir_all(folder).unwrap();
    }
    let folders: BTreeSet<PathBuf> = [chat.clone(), thread.clone(), nested.clone(), other, home.path().to_owned()].into();

    assert_eq!(chat_folders(home.path(), "chat-one", &folders), [chat, thread, nested]);
    assert!(chat_folders(home.path(), "chat-three", &folders).is_empty());
    assert!(chat_folders(home.path(), "..", &folders).is_empty());
    assert!(chat_folders(home.path(), "chat-one/pricing", &folders).is_empty());
}
