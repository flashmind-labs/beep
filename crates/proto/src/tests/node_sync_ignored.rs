use super::*;

#[test]
fn selection_is_explicit_relative_and_does_not_overlap() {
    for paths in [
        vec![],
        vec!["../secret"],
        vec!["/assets"],
        vec![".git/objects"],
        vec!["node_modules"],
        vec!["assets", "assets/models"],
        vec!["Models", "models"],
        vec!["x\\y"],
    ] {
        assert!(
            IgnoredSync {
                enabled: true,
                folders: paths.into_iter().map(str::to_owned).collect()
            }
            .normalized()
            .is_err()
        );
    }
    let config = IgnoredSync {
        enabled: true,
        folders: vec![" models/ ".into(), "models".into(), "data/captures".into()],
    }
    .normalized()
    .unwrap();
    assert_eq!(config.folders, ["data/captures", "models"]);
    assert_eq!(
        ignored_folder(&ignored_project("acme/widget", "data/captures")),
        Some(("acme/widget".into(), "data/captures"))
    );
    assert!(ignored_folder("@ignored/acme%2Fwidget/../outside").is_none());
}
