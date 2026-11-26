use super::*;
use std::fs;

#[test]
fn environment_includes_machine_project_and_gates_multi_machine_and_native_builds() {
    let single = environment_on(
        &Context {
            machine: Some("laptop".into()),
            project: None,
            multi_machine: false,
            rust_or_c: false,
        },
        "macos",
    );
    assert!(single.contains("running inside **Beep**"));
    assert!(single.contains("on `laptop` (macos)"));
    assert!(!single.contains("multi-machine mesh"));
    assert!(!single.contains("remote_run"));
    assert!(!single.contains("sccache"));

    let full = environment_on(
        &Context {
            machine: Some("build-box".into()),
            project: Some("beep".into()),
            multi_machine: true,
            rust_or_c: true,
        },
        "linux",
    );
    assert!(full.contains("on `build-box` (linux) in project `beep` as part of a multi-machine mesh"));
    assert!(full.contains("`remote_run` (never on `build-box`)"));
    assert!(full.contains("sccache"));
}

#[test]
fn chat_gates_image_titling_guidance() {
    let new_session = chat(true);
    assert!(new_session.contains("never `file://` URLs"));
    assert!(new_session.contains("beep://session/<id>"));
    assert!(new_session.contains("Session title"));

    let resumed_or_named = chat(false);
    assert!(resumed_or_named.contains("never `file://` URLs"));
    assert!(resumed_or_named.contains("beep://session/<id>"));
    assert!(!resumed_or_named.contains("Session title"));
}

#[test]
fn has_native_build_detects_markers_and_stops_at_git_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    assert!(!has_native_build(root));

    // A Cargo.toml above a nested git repository does not bleed into it.
    fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();
    let nested = root.join("nested");
    fs::create_dir_all(nested.join(".git")).unwrap();
    let sub = nested.join("src");
    fs::create_dir_all(&sub).unwrap();
    assert!(!has_native_build(&sub));

    // A marker inside the checkout is found from nested subdirectories.
    fs::write(nested.join("CMakeLists.txt"), "").unwrap();
    assert!(has_native_build(&sub));
}
