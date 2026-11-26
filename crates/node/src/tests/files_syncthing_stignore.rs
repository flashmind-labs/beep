use super::*;
use tempfile::TempDir;

/// The rules of `text`, for a project in `share/project`.
fn rules(text: &str) -> (TempDir, Vec<Stignore>) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("share/project");
    fs::create_dir_all(&root).unwrap();
    fs::write(dir.path().join("share").join(STIGNORE), text).unwrap();
    let found = Stignore::load_above(&root).unwrap();
    (dir, found)
}

fn ignored(rules: &[Stignore], path: &str) -> bool {
    rules.iter().any(|st| st.ignores(Path::new(path), false))
}

#[test]
fn a_rule_at_the_shares_root_reaches_a_project_below_it() {
    let (_dir, rules) = rules(
        "// a comment\n\
         (?d)/project/data/captures\n\
         /elsewhere\n\
         (?d)**/cache/*.bin\n\
         sub/dir\n\
         *.raw\n",
    );
    assert!(ignored(&rules, "data/captures"));
    assert!(ignored(&rules, "data/captures/2026/a.mov"));
    assert!(!ignored(&rules, "data/other.mov"));
    assert!(!ignored(&rules, "elsewhere"));
    assert!(ignored(&rules, "a/cache/x.bin"));
    // Without a leading `/`, at any depth, even with a `/` inside.
    assert!(ignored(&rules, "sub/dir/file"));
    assert!(ignored(&rules, "a/b/sub/dir/file"));
    assert!(ignored(&rules, "deep/take.raw"));
}

#[test]
fn the_first_match_wins_and_case_folds_per_line() {
    let (_dir, rules) = rules("!keep.log\n*.log\n(?i)Secret\n(?d)(?i)!Plain.txt\nplain*\n");
    assert!(!ignored(&rules, "keep.log"));
    assert!(ignored(&rules, "other.log"));
    assert!(ignored(&rules, "SECRET"));
    assert!(ignored(&rules, "secret/x"));
    assert!(!ignored(&rules, "plain.TXT"));
    assert!(ignored(&rules, "plain.other"));
}

#[test]
fn a_line_starting_with_a_hash_is_a_comment() {
    let (_dir, rules) = rules("# build output\n*.raw\n#also a note\n");
    assert!(ignored(&rules, "a.raw"));
    assert!(!ignored(&rules, "build output"));
}

#[test]
fn an_ignored_dataset_under_a_beep_owned_project_keeps_user_exclusions() {
    let dir = TempDir::new().unwrap();
    let project = dir.path().join("owner/widget/models");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        dir.path().join(STIGNORE),
        format!("{BEGIN}\n/owner/widget\n{END}\n*.tmp\n"),
    )
    .unwrap();
    let rules = Stignore::load_above(&project).unwrap();
    assert!(!ignored(&rules, "scene.blend"));
    assert!(ignored(&rules, "cache.tmp"));
}

#[test]
fn a_project_exclusion_hands_ownership_to_beep_and_keeps_user_rules() {
    let dir = TempDir::new().unwrap();
    let project = dir.path().join("owner/widget");
    let other = dir.path().join("owner/other");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&other).unwrap();
    fs::write(
        dir.path().join(STIGNORE),
        format!("{BEGIN}\n/owner/widget\n{END}\n*.tmp\n"),
    )
    .unwrap();
    assert!(project_ignored(&project, dir.path()).unwrap());
    assert!(!project_ignored(&other, dir.path()).unwrap());
    let rules = Stignore::load_above(&project).unwrap();
    assert!(!ignored(&rules, "src/main.rs"));
    assert!(ignored(&rules, "cache.tmp"));
    // A matching rule written by the user still belongs to the user.
    fs::write(
        dir.path().join(STIGNORE),
        format!("{BEGIN}\n/owner/widget\n{END}\n/owner/widget\n"),
    )
    .unwrap();
    assert!(ignored(
        &Stignore::load_above(&project).unwrap(),
        "src/main.rs"
    ));
}

#[test]
fn one_managed_block_holds_several_projects() {
    let dir = TempDir::new().unwrap();
    let project = dir.path().join("owner/widget");
    let other = dir.path().join("owner/other");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&other).unwrap();
    fs::write(
        dir.path().join(STIGNORE),
        format!("{BEGIN}\n/owner/widget\n/owner/other\n{END}\n"),
    )
    .unwrap();
    assert!(project_ignored(&project, dir.path()).unwrap());
    assert!(project_ignored(&other, dir.path()).unwrap());
    assert!(!ignored(&Stignore::load_above(&other).unwrap(), "a.rs"));
}

#[test]
fn exceptions_and_rules_outside_the_share_cannot_release_ownership() {
    let dir = TempDir::new().unwrap();
    let share = dir.path().join("share");
    let project = share.join("widget");
    fs::create_dir_all(&project).unwrap();
    fs::write(dir.path().join(STIGNORE), "/share\n").unwrap();
    assert!(!project_ignored(&project, &share).unwrap());
    fs::write(share.join(STIGNORE), "!/widget/keep.txt\n/widget\n").unwrap();
    assert!(!project_ignored(&project, &share).unwrap());
    fs::write(share.join(STIGNORE), "/**\n").unwrap();
    assert!(project_ignored(&share, &share).unwrap());
}

#[test]
fn an_include_counts_where_it_stands() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("project");
    fs::create_dir_all(&root).unwrap();
    fs::write(dir.path().join("more.txt"), "!kept.raw\n").unwrap();
    fs::write(root.join(STIGNORE), "#include ../more.txt\n*.raw\n").unwrap();
    let rules = Stignore::load_above(&root).unwrap();
    assert!(!ignored(&rules, "kept.raw"));
    assert!(ignored(&rules, "other.raw"));
}

#[test]
fn rules_it_cannot_follow_are_refused_and_say_why() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("project");
    fs::create_dir_all(&root).unwrap();
    for (text, says) in [
        ("(?x)odd\n", "(?x)odd"),
        ("#escape=|\n", "#escape"),
        ("#include missing.txt\n", "missing.txt"),
        ("#include .stignore\n", "goes around"),
        ("[broken\n", "[broken"),
    ] {
        fs::write(root.join(STIGNORE), text).unwrap();
        let error = format!("{:#}", Stignore::load_above(&root).unwrap_err());
        assert!(error.contains(says), "{text}: {error}");
        assert!(error.contains("does not take over"), "{error}");
    }
}
