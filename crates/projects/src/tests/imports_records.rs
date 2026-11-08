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
    assert_eq!(
        project_of(&machine(1), "/srv/work/widget", &imports),
        "acme/widget"
    );
    assert_eq!(
        project_of(&machine(1), "/srv/work/widget/.worktrees/fix", &imports),
        "acme/widget"
    );
    assert_eq!(
        project_of(&machine(1), "/srv/work/widget/vendor/relay/src", &imports),
        "acme/relay"
    );
    assert_eq!(
        project_of(&machine(1), "/srv/work/widgets", &imports),
        "widgets"
    );
    assert_eq!(
        project_of(&machine(2), "/srv/work/widget", &imports),
        "widget"
    );
}
