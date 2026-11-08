use super::*;

#[test]
fn only_a_folder_that_is_there_opens() {
    let dir = std::env::temp_dir();
    assert!(openable(&dir));
    assert!(!openable(Path::new("relative/folder")));
    assert!(!openable(&dir.join("no-such-folder-for-beep")));
}

#[test]
fn refusals_read_as_the_servers_sentence() {
    let old = DesktopError::Api(ApiError::bad_request("unknown op 166"));
    assert!(outdated(old).to_string().contains("needs an update"));
    let said = DesktopError::Api(ApiError::bad_request("build-box is offline."));
    assert_eq!(outdated(said).to_string(), "build-box is offline.");
}
