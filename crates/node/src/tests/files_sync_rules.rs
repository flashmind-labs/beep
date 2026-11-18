use super::*;
use tempfile::TempDir;

fn project(files: &[&str]) -> TempDir {
    let dir = TempDir::new().expect("a temp folder");

    for file in files {
        let path = dir.path().join(file);

        if let Some(stripped) = file.strip_suffix('/') {
            fs::create_dir_all(dir.path().join(stripped)).unwrap();
            continue;
        }

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }

    dir
}

fn skipped(rules: &Rules, path: &str, is_dir: bool) -> Option<Rule> {
    match rules.skip(Path::new(path), is_dir) {
        Some(Skip::Rule { rule, .. }) => Some(rule),
        Some(Skip::Own) => Some(Rule::Other),
        None => None,
    }
}

#[test]
fn a_marker_one_folder_down_counts_and_markers_combine() {
    let dir = project(&["app/package.json", "Cargo.toml", "deep/er/pyproject.toml"]);
    let rules = Rules::load(dir.path(), "").unwrap();
    assert_eq!(rules.markers(), ["Cargo.toml", "package.json"]);
    assert_eq!(
        skipped(&rules, "app/node_modules", true),
        Some(Rule::Default)
    );
    assert_eq!(skipped(&rules, "target", true), Some(Rule::Default));
    // Two folders down is too deep for a marker.
    assert_eq!(skipped(&rules, "build", true), None);
    let Some(Skip::Rule { marker, .. }) = rules.skip(Path::new("target"), true) else {
        panic!("target is left out");
    };
    assert_eq!(marker.as_deref(), Some("Cargo.toml"));
}

#[test]
fn only_the_roots_git_syncs_and_own_partials_are_hidden() {
    let dir = project(&["notes.txt"]);
    let rules = Rules::load(dir.path(), "").unwrap();
    assert_eq!(skipped(&rules, ".git", true), None);
    assert_eq!(skipped(&rules, ".git/objects/pack", true), None);
    assert_eq!(skipped(&rules, ".git/HEAD", false), None);
    for file in [
        ".git/index",
        ".git/index.lock",
        ".git/FETCH_HEAD",
        ".git/ORIG_HEAD",
        ".git/hooks/pre-commit",
        ".git/refs/heads/main.lock",
        ".git/objects/pack/tmp_pack_x",
    ] {
        assert_eq!(skipped(&rules, file, false), Some(Rule::Other), "{file}");
    }
    for folder in [".git/logs", ".git/worktrees", ".git/hooks"] {
        assert_eq!(skipped(&rules, folder, true), Some(Rule::Other), "{folder}");
    }

    assert_eq!(skipped(&rules, "sub/.git", false), Some(Rule::Git));
    assert_eq!(skipped(&rules, "sub/.git/config", false), Some(Rule::Git));
    assert_eq!(
        rules.skip(Path::new(".beep-1-2.partial"), true),
        Some(Skip::Own)
    );
    assert_eq!(skipped(&rules, "notes.txt", false), None);
}

#[test]
fn generic_build_output_and_syncthings_files_stay_in_any_project() {
    let dir = project(&["notes.txt"]);
    let rules = Rules::load(dir.path(), "").unwrap();
    for path in [
        "target",
        "app/node_modules",
        ".DS_Store",
        "a/__pycache__",
        "a/b/.cache",
        "mobile/android/build",
        "mobile/android/app/build",
        "mobile/ios/build",
        ".worktrees",
        "sub/.claude/worktrees",
        "notes.sync-conflict-20260101-000000-ABCDEFG.txt",
    ] {
        assert_eq!(skipped(&rules, path, true), Some(Rule::Default), "{path}");
    }
    assert_eq!(skipped(&rules, "src/build", true), None);
    for own in [
        ".stfolder",
        ".stignore",
        ".syncthing.a.tmp",
        "~syncthing~a.tmp",
    ] {
        assert_eq!(rules.skip(Path::new(own), false), Some(Skip::Own), "{own}");
    }
}

#[test]
fn editor_and_os_temporary_files_stay_out() {
    let dir = project(&["notes.txt"]);
    let rules = Rules::load(dir.path(), "").unwrap();
    for name in [
        ".a.txt.swp",
        "a.txt~",
        "4913",
        ".#a.txt",
        "#a.txt#",
        "a.rs___jb_tmp___",
        "Thumbs.db",
        "._a.txt",
        "a.tmp",
        "a.zip.crdownload",
        "a.part",
    ] {
        for path in [name.to_owned(), format!("sub/{name}")] {
            assert_eq!(skipped(&rules, &path, false), Some(Rule::Default), "{path}");
        }
    }
    assert_eq!(skipped(&rules, "notes.txt", false), None);
}

#[test]
fn a_path_inside_a_skipped_folder_is_skipped() {
    let dir = project(&["Cargo.toml"]);
    let rules = Rules::load(dir.path(), "").unwrap();
    assert!(rules.skips_any(Path::new("target/debug/app"), false));
    assert!(rules.skips_any(Path::new("crates/x/target"), true));
    assert!(rules.skips_any(Path::new(".git/index"), false));
    assert!(!rules.skips_any(Path::new(".git/refs/heads/main"), false));
    assert!(!rules.skips_any(Path::new("src/main.rs"), false));
}

#[test]
fn a_gitignore_outside_git_is_not_read() {
    let dir = project(&["Cargo.toml", ".env", "run.log"]);
    fs::write(dir.path().join(".gitignore"), ".env\n*.log\ntarget/\n").unwrap();
    let rules = Rules::load(dir.path(), "").unwrap();
    assert_eq!(skipped(&rules, ".env", false), None);
    assert_eq!(skipped(&rules, "run.log", false), None);
    assert_eq!(skipped(&rules, "target", true), Some(Rule::Default));
}

#[test]
fn beepignore_adds_rules_and_keeps_what_others_leave_out() {
    let dir = project(&["Cargo.toml", "package.json"]);
    fs::write(dir.path().join(".beepignore"), "renders/\n!dist/\n*.tmp\n").unwrap();
    let rules = Rules::load(dir.path(), "").unwrap();
    assert_eq!(skipped(&rules, "renders", true), Some(Rule::BeepIgnore));
    assert_eq!(skipped(&rules, "a.tmp", false), Some(Rule::BeepIgnore));
    // `!dist/` keeps the folder the package.json defaults leave out.
    assert_eq!(skipped(&rules, "dist", true), None);
    assert_eq!(skipped(&rules, "target", true), Some(Rule::Default));

    fs::write(dir.path().join(".beepignore"), "!target/\n").unwrap();
    let rules = Rules::load(dir.path(), "").unwrap();
    assert_eq!(skipped(&rules, "target", true), None);
}

#[test]
fn a_tracked_file_stays_in_spite_of_the_defaults() {
    let dir = project(&["package.json", "dist/kept.js", "dist/built.js", "target/a"]);
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(status.status.success(), "{status:?}");
    };
    git(&["init", "-q"]);
    git(&["add", "-f", "package.json", "dist/kept.js"]);
    let rules = Rules::load(dir.path(), "").unwrap();
    assert_eq!(skipped(&rules, "dist", true), None);
    assert_eq!(skipped(&rules, "dist/kept.js", false), None);
    assert_eq!(skipped(&rules, "dist/built.js", false), Some(Rule::Default));
    assert_eq!(skipped(&rules, "target", true), Some(Rule::Default));
}

#[test]
fn a_git_file_or_a_rule_keeps_git_here() {
    let dir = project(&["notes.txt"]);
    fs::write(dir.path().join(".git"), "gitdir: /path/to/work\n").unwrap();
    let rules = Rules::load(dir.path(), "").unwrap();
    assert!(!rules.syncs_git());
    assert_eq!(skipped(&rules, ".git", false), Some(Rule::Git));

    let dir = project(&["notes.txt", ".git/HEAD"]);
    fs::write(dir.path().join(".beepignore"), ".git/\n").unwrap();
    let rules = Rules::load(dir.path(), "").unwrap();
    assert!(!rules.syncs_git());
    assert_eq!(skipped(&rules, ".git/HEAD", false), Some(Rule::BeepIgnore));
}

#[test]
fn mesh_rules_follow_the_beepignore() {
    let dir = project(&["Cargo.toml"]);
    fs::write(dir.path().join(BEEPIGNORE), "*.log\n").unwrap();
    let rules = Rules::load(dir.path(), "datasets/\n!keep.log\n!target/release/app").unwrap();
    assert_eq!(skipped(&rules, "datasets", true), Some(Rule::BeepIgnore));
    assert_eq!(skipped(&rules, "run.log", false), Some(Rule::BeepIgnore));
    assert_eq!(skipped(&rules, "keep.log", false), None);
    assert_eq!(skipped(&rules, "target/release/app", false), None);
    // Without a .beepignore, the mesh rules alone apply.
    let bare = project(&["notes.txt"]);
    let rules = Rules::load(bare.path(), "notes.txt").unwrap();
    assert_eq!(skipped(&rules, "notes.txt", false), Some(Rule::BeepIgnore));
}
