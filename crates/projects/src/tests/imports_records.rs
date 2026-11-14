use super::*;
use iroh::SecretKey;

fn machine(seed: u8) -> EndpointId {
    SecretKey::from_bytes(&[seed; 32]).public()
}

fn import(folder: &str, project: &str) -> ImportRow {
    ImportRow {
        project: project.into(),
        endpoint: machine(1).to_string(),
        folder: folder.into(),
        url: String::new(),
        imported_at: DateTime::<Utc>::UNIX_EPOCH,
        fork: None,
    }
}

#[test]
fn sessions_go_to_the_imported_project_that_holds_them() {
    let imports = [
        import("/srv/work/widget", "acme/widget"),
        import("/srv/work/widget/vendor/relay", "acme/relay"),
    ];
    let project = |endpoint, cwd| imported_project_of(&machine(endpoint), cwd, &imports);
    assert_eq!(
        project(1, "/srv/work/widget").as_deref(),
        Some("acme/widget")
    );
    assert_eq!(
        project(1, "/srv/work/widget/.worktrees/fix").as_deref(),
        Some("acme/widget")
    );
    assert_eq!(
        project(1, "/srv/work/widget/vendor/relay/src").as_deref(),
        Some("acme/relay")
    );
    assert_eq!(project(1, "/srv/work/widgets"), None);
    assert_eq!(project(2, "/srv/work/widget"), None);
}
