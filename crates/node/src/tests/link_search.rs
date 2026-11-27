use super::*;
use crate::agents::Parts;
use crate::link::state::State;
use std::fs;

#[tokio::test]
async fn a_new_search_folder_is_saved_and_a_missing_one_refused() {
    let temp = tempfile::tempdir().expect("a temp folder");
    let dir = temp.path().join("node");
    State::load_or_start(&dir, "box").expect("the node's settings");
    // Walks this empty folder, not the home of whoever runs the test.
    let empty = temp.path().join("empty");
    fs::create_dir(&empty).expect("a folder");
    let agents = Arc::new(Agents::new(Parts::default()));
    let start = Search::from(empty.display().to_string().as_str());
    let searches = Searches::start(dir.clone(), agents, &start);
    let home = Reply::Folder {
        folder: "~/".into(),
    };
    assert_eq!(searches.answer(Request::Get {}).await, home);

    let missing = temp.path().join("missing").display().to_string();
    let refused = searches.answer(Request::Set { folder: missing }).await;
    let Reply::Failed { reason } = refused else {
        panic!("a missing folder is refused: {refused:?}");
    };
    assert!(reason.contains("is not a folder"), "{reason}");

    let projects = temp.path().join("projects");
    fs::create_dir(&projects).expect("a folder");
    let folder = projects.display().to_string();
    let set = Request::Set {
        folder: folder.clone(),
    };
    let saved = Reply::Folder { folder };
    assert_eq!(searches.answer(set).await, saved);
    assert_eq!(searches.answer(Request::Get {}).await, saved);
}
