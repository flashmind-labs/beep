use super::*;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::{env, fs, process};

fn place(id: &str, project: &str, folder: &str) -> Place {
    Place {
        project: project.into(),
        machine_id: id.into(),
        folder: folder.into(),
    }
}

#[test]
fn a_path_is_under_a_folder_only_at_a_name_boundary() {
    assert_eq!(under("/srv/app/a.glb", "/srv/app"), Some("a.glb"));
    assert_eq!(under("/srv/app", "/srv/app/"), Some(""));
    assert_eq!(under("/srv/apple/a.glb", "/srv/app"), None);
    assert_eq!(under("~/a.glb", "/srv/app"), None);
}

#[test]
fn a_session_folder_belongs_to_the_innermost_project_on_its_machine() {
    let imported = [
        place("box", "acme/all", "/srv"),
        place("box", "acme/widget", "/srv/widget"),
        place("mac", "acme/other", "/srv/widget/sub"),
    ];
    assert_eq!(
        holder("box", "/srv/widget/sub", &imported),
        Some(("acme/widget", "/srv/widget"))
    );
    assert_eq!(holder("box", "/opt/else", &imported), None);
}

/// What `find_copy` finds when this Mac has only the folders `here`.
fn found(dir: &str, places: &[Place], here: &[&str]) -> Result<LocalCopy, Miss> {
    find_copy("box", dir, places, |folder| {
        here.iter().any(|there| folder == Path::new(there))
    })
}

fn copy(remote: &str, local: &str) -> LocalCopy {
    LocalCopy {
        remote: remote.into(),
        local: local.into(),
    }
}

#[test]
fn a_project_imported_on_both_reads_here_whatever_the_case_or_slash() {
    let imported = [
        place("box", "Acme/Widget", "/srv/widget/"),
        place("mac", "acme/widget", "/mac/code/widget"),
    ];
    assert_eq!(
        found("/srv/widget", &imported, &["/mac/code/widget"]),
        Ok(copy("/srv/widget/", "/mac/code/widget"))
    );
}

#[test]
fn git_checkouts_read_from_the_remote_machine_even_at_the_same_commit() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    let local = dir.path().to_str().unwrap();
    let imported = [
        place("box", "acme/widget", "/srv/widget"),
        place("mac", "acme/widget", local),
    ];
    assert!(matches!(
        found("/srv/widget", &imported, &[local]),
        Err(Miss::NotHere { .. })
    ));
}

#[test]
fn a_project_synced_by_beep_only_reads_here() {
    let synced = [
        place("box", "acme/widget", "/srv/widget"),
        place("mac", "acme/widget", "/mac/widget"),
    ];
    assert_eq!(
        found("/srv/widget", &synced, &["/mac/widget"]),
        Ok(copy("/srv/widget", "/mac/widget"))
    );
}

#[test]
fn another_project_of_the_same_folder_name_is_not_the_copy() {
    let imported = [
        place("box", "acme/widget", "/srv/widget"),
        place("mac", "other/widget", "/mac/widget"),
    ];
    assert_eq!(
        found("/srv/widget", &imported, &["/mac/widget"]),
        Err(Miss::NotHere {
            project: "acme/widget".into(),
            tried: Vec::new(),
        })
    );
}

#[test]
fn a_session_in_a_subfolder_or_worktree_reads_under_the_projects_folder() {
    let imported = [
        place("box", "acme/widget", "/srv/widget"),
        place("mac", "acme/widget", "/mac/widget"),
    ];

    for dir in ["/srv/widget/assets", "/srv/widget/.worktrees/fix/assets"] {
        assert_eq!(
            found(dir, &imported, &["/mac/widget"]),
            Ok(copy("/srv/widget", "/mac/widget")),
            "{dir}"
        );
    }

    assert_eq!(
        under("/srv/widget/.worktrees/fix/a.glb", "/srv/widget"),
        Some(".worktrees/fix/a.glb")
    );
}

#[test]
fn a_folder_not_here_is_a_miss_that_names_where_it_looked() {
    let imported = [
        place("box", "acme/widget", "/srv/widget"),
        place("mac", "acme/widget", "/mac/widget"),
    ];
    assert_eq!(
        found("/srv/widget", &imported, &[]),
        Err(Miss::NotHere {
            project: "acme/widget".into(),
            tried: vec!["/mac/widget".into()],
        })
    );
}

#[test]
fn a_machine_that_lost_its_imports_holds_no_project() {
    let imported = [place("mac", "acme/widget", "/mac/widget")];
    assert_eq!(
        found("/srv/widget", &imported, &["/mac/widget"]),
        Err(Miss::NoProject)
    );
}

#[cfg(unix)]
#[test]
fn nothing_outside_the_local_folder_counts_as_inside_it() {
    let dir = env::temp_dir().join(format!("beep-work-copy-{}", process::id()));
    let (root, outside) = (dir.join("widget"), dir.join("outside"));
    fs::create_dir_all(root.join("assets")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, root.join("link")).unwrap();
    assert!(stays_inside(&root, &root));
    assert!(stays_inside(&root, &root.join("assets")));
    assert!(!stays_inside(&root, &root.join("link")));
    assert!(!stays_inside(&root, &root.join("missing")));
    assert!(!stays_inside(&root, &root.join("../outside")));
    // What a doubled slash in a session's path would join to.
    assert!(!stays_inside(&root, &root.join(&outside)));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_folder_that_did_not_sync_opens_the_top_of_the_copy() {
    let dir = env::temp_dir().join(format!("beep-work-copy-top-{}", process::id()));
    let local = dir.join("widget");
    fs::create_dir_all(local.join("sdk")).unwrap();
    assert_eq!(place_or_top(local.clone(), "sdk"), local.join("sdk"));
    assert_eq!(place_or_top(local.clone(), ".worktrees/fix"), local);
    assert_eq!(place_or_top(local.clone(), "../other"), local);
    fs::remove_dir_all(&dir).unwrap();
}
