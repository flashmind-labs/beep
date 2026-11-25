use super::*;
use crate::STATE_FILE;
use crate::managed::{GitOrigin, Policy};
use crate::swap::{InstallSwap, ManageError, clean_leftovers};

fn id() -> PluginId {
    "cat-one".parse().unwrap()
}

fn write(dir: &Path, version: &str, permissions: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("beep-plugin.json"),
        format!(
            r#"{{ "id": "cat-one", "name": "Cat", "version": "{version}",
                  "permissions": [{permissions}] }}"#
        ),
    )
    .unwrap();
    std::fs::write(dir.join("main.js"), "// plugin").unwrap();
}

fn git(commit: &str) -> Origin {
    Origin::Git(GitOrigin {
        url: "https://github.com/owner/repo.git".to_owned(),
        reference: None,
        subfolder: None,
        commit: commit.parse().unwrap(),
    })
}

fn managed(root: &Path) -> Managed {
    Managed::load(root, Policy::Managed)
}

#[test]
fn a_new_plugin_previews_all_its_permissions_as_added() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join(".stage"),
        "0.1.0",
        r#""ui.add", "sessions.read""#,
    );

    let shown = managed(root.path())
        .preview_install(root.path(), &root.path().join(".stage"), &git("aaaaaaa"))
        .unwrap();

    assert_eq!(shown.kind, PreviewKind::New);
    assert_eq!(shown.added, [Permission::UiAdd, Permission::SessionsRead]);
    assert!(shown.removed.is_empty());
    assert_eq!(shown.source, git("aaaaaaa"));
}

#[test]
fn installing_moves_the_folder_and_records_the_approval() {
    let root = tempfile::tempdir().unwrap();
    let staged = root.path().join(".stage");
    write(&staged, "0.1.0", r#""ui.add""#);
    let state = managed(root.path());

    let installed = state.install(&staged, git("aaaaaaa")).unwrap();

    assert_eq!(installed, id());
    assert!(!staged.exists());
    assert!(root.path().join("cat-one/main.js").is_file());
    let record = state.record(&id()).unwrap();
    assert!(record.enabled);
    assert_eq!(record.approved, [Permission::UiAdd]);
    assert_eq!(record.origin, git("aaaaaaa"));
}

#[test]
fn an_id_installed_from_another_source_is_refused() {
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join(".one"), "0.1.0", "");
    write(&root.path().join(".two"), "0.2.0", "");
    let state = managed(root.path());
    state
        .install(&root.path().join(".one"), Origin::Local)
        .unwrap();

    let refused = state.install(&root.path().join(".two"), git("aaaaaaa"));

    assert!(matches!(refused, Err(ManageError::AlreadyInstalled(found)) if found == id()));
    assert!(root.path().join(".two").is_dir());
}

#[test]
fn a_folder_without_a_record_counts_as_another_source() {
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join("cat-one"), "0.1.0", "");
    write(&root.path().join(".two"), "0.2.0", "");

    let refused = managed(root.path()).install(&root.path().join(".two"), Origin::Local);

    assert!(matches!(refused, Err(ManageError::AlreadyInstalled(_))));
}

#[test]
fn an_update_from_the_same_source_replaces_the_folder_and_shows_the_difference() {
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join(".one"), "0.1.0", r#""ui.add""#);
    write(&root.path().join(".two"), "0.2.0", r#""sessions.read""#);
    let state = managed(root.path());
    state
        .install(&root.path().join(".one"), git("aaaaaaa"))
        .unwrap();
    state
        .update(&id(), None, |record| record.enabled = false)
        .unwrap();

    let shown = state
        .preview_install(root.path(), &root.path().join(".two"), &git("bbbbbbb"))
        .unwrap();
    assert_eq!(shown.kind, PreviewKind::Update);
    assert_eq!(shown.previous_version, Some(semver::Version::new(0, 1, 0)));
    assert_eq!(shown.added, [Permission::SessionsRead]);
    assert_eq!(shown.removed, [Permission::UiAdd]);

    state
        .install(&root.path().join(".two"), git("bbbbbbb"))
        .unwrap();

    let manifest = Manifest::read(&root.path().join("cat-one")).unwrap();
    assert_eq!(manifest.version, semver::Version::new(0, 2, 0));
    let record = state.record(&id()).unwrap();
    assert_eq!(record.approved, [Permission::SessionsRead]);
    assert!(!record.enabled, "an update keeps a plugin off");
    assert_eq!(record.origin, git("bbbbbbb"));
    assert!(!root.path().join(".old-cat-one").exists());
}

#[test]
fn allow_previews_the_missing_permissions_of_an_installed_plugin() {
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join("cat-one"), "0.1.0", r#""ui.add""#);

    let shown = managed(root.path())
        .preview_allow(root.path(), &id())
        .unwrap();

    assert_eq!(shown.kind, PreviewKind::Allow);
    assert_eq!(shown.added, [Permission::UiAdd]);
    assert!(matches!(
        managed(root.path()).preview_allow(root.path(), &"nope-one".parse().unwrap()),
        Err(ManageError::Unknown(_))
    ));
}

#[test]
fn a_bad_manifest_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let staged = root.path().join(".stage");
    std::fs::create_dir_all(&staged).unwrap();
    std::fs::write(staged.join("beep-plugin.json"), "{").unwrap();

    let refused = managed(root.path()).install(&staged, Origin::Local);

    assert!(matches!(refused, Err(ManageError::Manifest(_))));
}

#[test]
fn a_folder_over_the_size_cap_is_refused() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "0.1.0", "");
    std::fs::write(root.path().join("big.bin"), vec![0u8; 2048]).unwrap();

    assert!(matches!(
        check_tree(root.path(), 1024),
        Err(ManageError::TooLarge)
    ));
    assert!(check_tree(root.path(), 4096).is_ok());
}

#[cfg(unix)]
#[test]
fn a_symlink_that_leaves_the_folder_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let plugin = root.path().join("plugin");
    write(&plugin, "0.1.0", "");
    std::fs::write(root.path().join("secret.txt"), "x").unwrap();
    std::os::unix::fs::symlink("../secret.txt", plugin.join("link.txt")).unwrap();

    let refused = check_tree(&plugin, MAX_PLUGIN_BYTES);

    assert!(matches!(refused, Err(ManageError::Symlink(path)) if path == Path::new("link.txt")));
}

#[cfg(unix)]
#[test]
fn a_symlink_inside_the_folder_and_a_broken_one_are_judged_by_their_target() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "0.1.0", "");
    std::os::unix::fs::symlink("main.js", root.path().join("alias.js")).unwrap();
    assert!(check_tree(root.path(), MAX_PLUGIN_BYTES).is_ok());

    std::os::unix::fs::symlink("missing.js", root.path().join("gone.js")).unwrap();
    assert!(matches!(
        check_tree(root.path(), MAX_PLUGIN_BYTES),
        Err(ManageError::Symlink(_))
    ));
}

#[test]
fn leftovers_of_a_crashed_update_are_cleaned() {
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join(".old-cat-one"), "0.1.0", "");
    write(&root.path().join("cat-one"), "0.2.0", "");

    clean_leftovers(root.path());

    assert!(!root.path().join(".old-cat-one").exists());
    assert!(root.path().join("cat-one").exists());
}

#[test]
fn a_legacy_backup_without_a_replacement_is_restored() {
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join(".old-cat-one"), "0.1.0", "");

    managed(root.path()).recover_installs().unwrap();

    assert!(!root.path().join(".old-cat-one").exists());
    assert!(root.path().join("cat-one/main.js").is_file());
}

#[test]
fn an_interrupted_update_recovers_the_original_folder_and_record() {
    for placed_replacement in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join(".stage");
        write(&old, "0.1.0", r#""ui.add""#);
        managed(root.path()).install(&old, git("aaaaaaa")).unwrap();
        let file = root.path().join(STATE_FILE);
        let mut state: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        state["pending_install"] = serde_json::json!({"id": "cat-one", "replacing": true});
        std::fs::write(&file, serde_json::to_vec(&state).unwrap()).unwrap();

        let swap = InstallSwap::new(root.path(), &id());
        std::fs::rename(&swap.target, &swap.backup).unwrap();
        if placed_replacement {
            write(&swap.target, "0.2.0", r#""sessions.read""#);
        }

        let recovered = managed(root.path());
        recovered.recover_installs().unwrap();
        recovered.recover_installs().unwrap();

        assert_eq!(
            Manifest::read(&swap.target).unwrap().version,
            semver::Version::new(0, 1, 0)
        );
        assert_eq!(
            recovered.record(&id()).unwrap().approved,
            [Permission::UiAdd]
        );
        assert_eq!(recovered.record(&id()).unwrap().origin, git("aaaaaaa"));
        assert!(!swap.backup.exists());
    }
}

#[test]
fn an_interrupted_first_install_removes_the_uncommitted_folder() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("cat-one");
    write(&target, "0.1.0", "");
    std::fs::write(
        root.path().join(STATE_FILE),
        br#"{
        "version": 1, "plugins": {},
        "pending_install": {"id": "cat-one", "replacing": false}
    }"#,
    )
    .unwrap();

    let recovered = managed(root.path());
    recovered.recover_installs().unwrap();

    assert!(!target.exists());
    assert!(recovered.record(&id()).is_none());
}
