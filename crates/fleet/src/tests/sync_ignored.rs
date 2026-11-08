use super::*;
use beep_proto::node::sync::ignored_project;

#[test]
fn ignored_content_is_counted_and_conflicts_keep_the_selected_folder_prefix() {
    let main = ProjectStatus {
        name: "acme/widget".into(),
        state: State::Idle,
        git_only: true,
        ..ProjectStatus::default()
    };
    let config = IgnoredSync {
        enabled: true,
        folders: vec!["models".into()],
    };
    let child = ProjectStatus {
        name: ignored_project(&main.name, "models"),
        state: State::Idle,
        files: 3,
        bytes: 700,
        conflicts: vec!["scene (conflict abcdef01).blend".into()],
        ..ProjectStatus::default()
    };
    let status = config.status(&main, &[child]);
    assert_eq!(status.files, 3);
    assert_eq!(status.conflicts, ["models/scene (conflict abcdef01).blend"]);
    assert!(status.git_only);
    assert_eq!(config.status(&main, &[]).state, State::Failed);
}
