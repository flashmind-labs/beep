use super::*;

fn manifest(permissions: &[&str]) -> Manifest {
    let list = permissions
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(",");

    Manifest::parse(&format!(
        r#"{{ "id": "cat-one", "name": "Cat", "version": "0.1.0", "permissions": [{list}] }}"#
    ))
    .unwrap()
}

fn id() -> PluginId {
    "cat-one".parse().unwrap()
}

fn git_origin() -> Origin {
    Origin::Git(GitOrigin {
        url: "https://github.com/owner/repo.git".to_owned(),
        reference: Some("main".to_owned()),
        subfolder: Some(PathBuf::from("plugins/cat")),
        commit: "0123456789abcdef0123456789abcdef01234567".parse().unwrap(),
    })
}

#[test]
fn records_round_trip_through_the_file() {
    let root = tempfile::tempdir().unwrap();
    let managed = Managed::load(root.path(), Policy::Managed);
    managed
        .update(&id(), None, |record| {
            record.approved = vec![Permission::UiAdd];
            record.origin = git_origin();
        })
        .unwrap();

    let again = Managed::load(root.path(), Policy::Managed);

    let record = again.record(&id()).unwrap();
    assert!(record.enabled);
    assert_eq!(record.approved, [Permission::UiAdd]);
    assert_eq!(record.origin, git_origin());
}

#[test]
fn the_file_has_a_stable_shape() {
    let root = tempfile::tempdir().unwrap();
    let managed = Managed::load(root.path(), Policy::Managed);
    managed
        .update(&id(), None, |record| {
            record.approved = vec![Permission::UiAdd];
            record.origin = git_origin();
        })
        .unwrap();

    let text = std::fs::read_to_string(root.path().join(STATE_FILE)).unwrap();

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).unwrap(),
        serde_json::json!({
            "version": 1,
            "plugins": { "cat-one": {
                "enabled": true,
                "approved": ["ui.add"],
                "order": 1,
                "origin": { "kind": "git", "url": "https://github.com/owner/repo.git",
                            "ref": "main", "subfolder": "plugins/cat",
                            "commit": "0123456789abcdef0123456789abcdef01234567" }
            } }
        })
    );
}

#[test]
fn a_corrupt_file_reads_as_empty_and_is_replaced_by_the_next_write() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(STATE_FILE), "{ not json").unwrap();

    let managed = Managed::load(root.path(), Policy::Managed);
    assert!(managed.record(&id()).is_none());
    managed.update(&id(), None, |_| {}).unwrap();

    let again = Managed::load(root.path(), Policy::Managed);
    assert!(again.record(&id()).is_some());
}

#[test]
fn a_write_leaves_no_temporary_file() {
    let root = tempfile::tempdir().unwrap();
    let managed = Managed::load(root.path(), Policy::Managed);

    managed.update(&id(), None, |_| {}).unwrap();
    managed.forget(&id()).unwrap();

    let mut names: Vec<String> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, [STATE_FILE]);
}

#[test]
fn a_failed_write_keeps_the_old_record() {
    let root = tempfile::tempdir().unwrap();
    let managed = Managed::load(root.path(), Policy::Managed);
    managed.update(&id(), None, |_| {}).unwrap();
    // A folder where the state file goes makes the rename fail.
    std::fs::remove_file(root.path().join(STATE_FILE)).unwrap();
    std::fs::create_dir(root.path().join(STATE_FILE)).unwrap();

    let result = managed.update(&id(), None, |record| record.enabled = false);

    assert!(result.is_err());
    assert!(managed.record(&id()).unwrap().enabled);
    assert!(!root.path().join(".plugins.json.tmp").exists());
}

#[test]
fn a_managed_folder_waits_for_approval_without_a_record() {
    let root = tempfile::tempdir().unwrap();
    let managed = Managed::load(root.path(), Policy::Managed);

    let assessed = managed.assess(&id(), &manifest(&["ui.add"]));

    assert_eq!(
        assessed.verdict,
        Verdict::NeedsApproval(vec![Permission::UiAdd])
    );
    assert_eq!(assessed.origin, Origin::Local);
}

#[test]
fn a_dev_folder_runs_without_a_record() {
    let root = tempfile::tempdir().unwrap();
    let managed = Managed::load(root.path(), Policy::Dev);

    let assessed = managed.assess(&id(), &manifest(&["ui.add", "sessions.read"]));

    assert_eq!(assessed.verdict, Verdict::Run);
    assert_eq!(assessed.origin, Origin::Dev);
}

#[test]
fn a_plugin_without_permissions_runs_once_it_has_a_record() {
    let root = tempfile::tempdir().unwrap();
    let managed = Managed::load(root.path(), Policy::Managed);

    assert_eq!(managed.assess(&id(), &manifest(&[])).verdict, Verdict::Run);
}

#[test]
fn a_disabled_record_does_not_run() {
    let root = tempfile::tempdir().unwrap();
    let managed = Managed::load(root.path(), Policy::Managed);
    managed
        .update(&id(), None, |record| {
            record.enabled = false;
            record.approved = vec![Permission::UiAdd];
        })
        .unwrap();

    let assessed = managed.assess(&id(), &manifest(&["ui.add"]));

    assert_eq!(assessed.verdict, Verdict::Disabled);
    assert!(!assessed.enabled);
}

#[test]
fn a_new_permission_needs_approval_even_in_a_dev_folder_with_a_record() {
    let root = tempfile::tempdir().unwrap();
    let managed = Managed::load(root.path(), Policy::Dev);
    managed
        .update(&id(), Some(&manifest(&["ui.add"])), |_| {})
        .unwrap();

    let assessed = managed.assess(&id(), &manifest(&["ui.add", "sessions.read"]));

    assert_eq!(
        assessed.verdict,
        Verdict::NeedsApproval(vec![Permission::SessionsRead])
    );
}

#[test]
fn commits_are_hex_and_shorten_to_seven_digits() {
    let commit: Commit = "0123456789ABCDEF".parse().unwrap();

    assert_eq!(commit.short(), "0123456");
    assert_eq!(commit.to_string(), "0123456789abcdef");
    for bad in ["", "abc", "xyz1234", "01234 6"] {
        assert!(bad.parse::<Commit>().is_err(), "{bad:?}");
    }
}

#[test]
fn only_the_same_source_may_update() {
    let other = Origin::Git(GitOrigin {
        url: "https://github.com/owner/other.git".to_owned(),
        ..match git_origin() {
            Origin::Git(origin) => origin,
            _ => unreachable!(),
        }
    });

    assert!(git_origin().accepts_update_from(&git_origin()));
    assert!(Origin::Local.accepts_update_from(&Origin::Local));
    assert!(!git_origin().accepts_update_from(&other));
    assert!(!git_origin().accepts_update_from(&Origin::Local));
    assert!(!Origin::Dev.accepts_update_from(&Origin::Dev));
}
