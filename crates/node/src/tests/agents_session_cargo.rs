use super::*;
use crate::agents::checkout::tests::{repo, run};

#[test]
fn cargo_wrapper_and_uncertain_configurations_are_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let configured = |path: &Path| {
        super::configured(
            path,
            "rustc-wrapper",
            &["RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER"],
        )
    };
    assert!(!configured(&config));
    fs::write(&config, "[build]\njobs = 2\n").unwrap();
    assert!(!configured(&config));
    fs::write(&config, "[build]\nrustc-wrapper = 'other'\n").unwrap();
    assert!(configured(&config));
    fs::write(&config, "[env]\nRUSTC_WRAPPER = 'other'\n").unwrap();
    assert!(configured(&config));
    fs::write(&config, "include = 'other.toml'\n").unwrap();
    assert!(configured(&config));
    fs::write(&config, "[invalid").unwrap();
    assert!(configured(&config));
}

#[test]
fn rust_checkouts_and_their_worktrees_share_one_target() {
    let (_temp, root) = repo("cargo-target");
    fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
    run(&root, &["add", "Cargo.toml"]);
    run(&root, &["commit", "--quiet", "-m", "Add workspace"]);
    run(&root, &["worktree", "add", "--quiet", ".worktrees/change"]);
    let expected = dunce::canonicalize(&root).unwrap().join("target");

    for cwd in [&root, &root.join("src"), &root.join(".worktrees/change")] {
        assert_eq!(project_target(cwd), Some(expected.clone()));
    }
}

#[test]
fn plain_rust_folders_use_the_manifest_folder_and_other_folders_stay_alone() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("src")).unwrap();
    assert_eq!(project_target(root), None);
    fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
    let expected = dunce::canonicalize(root).unwrap().join("target");
    assert_eq!(project_target(&root.join("src")), Some(expected));
}

#[test]
fn non_rust_worktrees_do_not_use_the_parent_checkouts_manifest() {
    let (_temp, root) = repo("non-rust-target");
    fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
    run(&root, &["worktree", "add", "--quiet", ".worktrees/change"]);
    assert_eq!(project_target(&root.join(".worktrees/change")), None);
}

#[test]
fn explicit_target_environment_is_preserved() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("Cargo.toml"), "[workspace]\n").unwrap();

    for name in ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"] {
        let overrides = BTreeMap::from([(name.to_owned(), "custom-output".to_owned())]);
        assert_eq!(target_dir(temp.path(), &overrides), None);
    }
}

#[test]
fn target_configuration_and_uncertain_configurations_are_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config.toml");
    let configured = |path: &Path| {
        super::configured(
            path,
            "target-dir",
            &["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"],
        )
    };

    for text in [
        "[build]\ntarget-dir = 'custom-output'\n",
        "[env]\nCARGO_TARGET_DIR = 'custom-output'\n",
        "[env.CARGO_BUILD_TARGET_DIR]\nvalue = 'custom-output'\nforce = true\n",
        "include = 'other.toml'\n",
        "[invalid",
    ] {
        fs::write(&config, text).unwrap();
        assert!(configured(&config), "{text}");
    }

    fs::write(&config, "[build]\nrustc-wrapper = 'other'\n").unwrap();
    assert!(!configured(&config));
}

#[test]
fn cargo_configuration_is_found_in_ancestors_and_cargo_home() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("src/deep")).unwrap();
    fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
    let overrides = BTreeMap::from([("CARGO_HOME".to_owned(), "cargo-home".to_owned())]);

    for dir in [root.join(".cargo"), root.join("src/deep/cargo-home")] {
        fs::create_dir_all(&dir).unwrap();

        for name in ["config", "config.toml"] {
            let config = dir.join(name);
            fs::write(&config, "[build]\ntarget-dir = 'custom-output'\n").unwrap();
            assert_eq!(target_dir(&root.join("src/deep"), &overrides), None);
            fs::remove_file(config).unwrap();
        }
    }
}
