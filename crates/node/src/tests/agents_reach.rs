use super::*;
use crate::agents::{Agents, Parts};
use std::fs;
use tempfile::TempDir;

#[test]
fn a_missing_path_resolves_through_its_folder() {
    let dir = TempDir::new().unwrap();
    let base = paths::canonicalize(dir.path()).unwrap();
    let asked = dir.path().join("new/deeper/file.mp4");
    assert_eq!(real(&asked).unwrap(), base.join("new/deeper/file.mp4"));
    let up = dir.path().join("a/../../x.png");
    assert!(real(&up).is_err());
}

#[test]
fn reads_reach_further_than_writes() {
    let work = TempDir::new().unwrap();
    let logs = TempDir::new().unwrap();
    let reach = Reached {
        work: vec![work.path().to_owned()],
        read: vec![logs.path().to_owned()],
        no_write: Vec::new(),
        sessions: Vec::new(),
    };
    let in_logs = real(&logs.path().join("a.jsonl")).unwrap();
    assert!(reach.holds(&in_logs, Access::Read));
    assert!(!reach.holds(&in_logs, Access::Write));
    let in_work = real(&work.path().join("src")).unwrap();
    assert!(reach.holds(&in_work, Access::Write));
    assert!(!reach.holds(Path::new("/etc"), Access::Read));
}

/// `asked` checked as `reachable` does, without the sessions found in
/// the harnesses' logs.
fn check(reach: &Reached, asked: &Path, access: Access) -> Result<PathBuf> {
    let asked = asked.to_str().expect("temp paths are UTF-8");
    let found = real(&resolve(asked, None)?)?;
    if reach.holds(&found, access) {
        Ok(found)
    } else {
        Err(refused(asked, access))
    }
}

fn project_reach(project: &TempDir) -> Reached {
    Reached {
        work: vec![project.path().to_owned()],
        ..Reached::default()
    }
}

#[test]
fn a_path_outside_is_refused() {
    let project = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("secret.png"), "secret").unwrap();
    let reach = project_reach(&project);

    for access in [Access::Read, Access::Write] {
        let e = check(&reach, &outside.path().join("secret.png"), access).unwrap_err();
        assert!(e.to_string().contains("outside the folders"), "{e:#}");
    }

    let e = check(&reach, Path::new("/etc/passwd"), Access::Read).unwrap_err();
    assert!(e.to_string().contains("outside the folders"), "{e:#}");
}

#[test]
fn a_dot_dot_escape_is_refused() {
    let project = TempDir::new().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    let reach = project_reach(&project);
    let escape = project.path().join("src/../../etc");
    assert!(check(&reach, &escape, Access::Read).is_err());
    let missing = project.path().join("gone/../../../etc/passwd");
    assert!(check(&reach, &missing, Access::Read).is_err());
}

#[cfg(unix)]
#[test]
fn a_symlink_escape_is_refused() {
    let project = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("id_key"), "secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), project.path().join("out")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("id_key"),
        project.path().join("key.png"),
    )
    .unwrap();
    let reach = project_reach(&project);

    for asked in ["out", "out/id_key", "key.png", "out/not-yet.png"] {
        let e = check(&reach, &project.path().join(asked), Access::Read).unwrap_err();
        assert!(
            e.to_string().contains("outside the folders"),
            "{asked}: {e:#}"
        );
    }
}

/// A reach with `temp` as Claude Code's temp folder, which a session
/// also works in.
fn temp_reach(temp: &TempDir) -> Reached {
    Reached {
        work: vec![temp.path().to_owned()],
        read: vec![temp.path().to_owned()],
        no_write: vec![temp.path().to_owned()],
        sessions: Vec::new(),
    }
}

#[cfg(unix)]
#[test]
fn a_symlink_out_of_claude_codes_temp_folder_is_refused() {
    let temp = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("notes.txt"), "secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), temp.path().join("link")).unwrap();
    let reach = temp_reach(&temp);
    let e = check(&reach, &temp.path().join("link/notes.txt"), Access::Read).unwrap_err();
    assert!(e.to_string().contains("outside the folders"), "{e:#}");
}

#[test]
fn claude_codes_temp_folder_is_never_written() {
    let temp = TempDir::new().unwrap();
    fs::create_dir(temp.path().join("scratchpad")).unwrap();
    let reach = temp_reach(&temp);
    let e = check(&reach, &temp.path().join("scratchpad"), Access::Write).unwrap_err();
    assert!(e.to_string().contains("may write"), "{e:#}");
    // Read only: the same folder reads.
    assert!(check(&reach, &temp.path().join("scratchpad"), Access::Read).is_ok());
}

#[cfg(unix)]
#[test]
fn claude_codes_temp_folder_is_in_tmp_by_uid() {
    use std::ffi::OsStr;
    // Wherever TMPDIR points, as it does under launchd on macOS.
    assert_eq!(claude_temp_in(None, 501), Path::new("/tmp/claude-501"));
    assert_eq!(
        claude_temp_in(Some(OsStr::new("")), 1000),
        Path::new("/tmp/claude-1000")
    );
    assert_eq!(
        claude_temp_in(Some(OsStr::new("/srv/scratch")), 501),
        Path::new("/srv/scratch/claude-501")
    );
    let uid = rustix::process::getuid().as_raw();
    let set = std::env::var_os(CLAUDE_TEMP_VAR);
    assert_eq!(claude_temp(), Some(claude_temp_in(set.as_deref(), uid)));
}

#[tokio::test]
async fn a_shared_folder_is_reached_through_the_agents() {
    let project = TempDir::new().unwrap();
    fs::write(project.path().join("model.glb"), "glTF").unwrap();
    let agents = Agents::new(Parts {
        folders: vec![project.path().to_owned()],
        ..Parts::default()
    });
    let asked = project.path().join("model.glb");
    let real = agents
        .reach
        .reachable(asked.to_str().unwrap(), Access::Write)
        .await
        .unwrap();
    assert_eq!(real, paths::canonicalize(&asked).unwrap());
}

#[test]
fn custom_agents_are_listed_only_in_a_work_folder() {
    let project = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let logs = TempDir::new().unwrap();
    fs::create_dir_all(project.path().join(".claude/agents")).unwrap();
    fs::write(
        project.path().join(".claude/agents/reviewer.md"),
        "---\nname: reviewer\n---\nReview.",
    )
    .unwrap();
    fs::create_dir_all(outside.path().join(".claude/agents")).unwrap();
    let reach = Reached {
        work: vec![project.path().to_owned()],
        read: vec![logs.path().to_owned()],
        no_write: Vec::new(),
        sessions: Vec::new(),
    };
    let real = check(&reach, project.path(), Access::Work).unwrap();
    let agents = crate::custom_agents::list(&real, None).unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].name, "reviewer");

    for refused_folder in [outside.path(), logs.path(), Path::new("/etc")] {
        let e = check(&reach, refused_folder, Access::Work).unwrap_err();
        assert!(e.to_string().contains("outside the folders"), "{e:#}");
    }

    assert!(check(&reach, Path::new("relative/path"), Access::Work).is_err());
}

#[test]
fn a_session_in_a_removed_worktree_reaches_its_checkout_only() {
    let (tmp, main, gone) =
        super::super::worktrees::tests::repo_with_removed_worktree(".worktrees");
    let reach = |folder: &Path| Reached {
        sessions: vec![folder.to_owned()],
        ..Reached::default()
    };
    let before = reach(&gone);
    assert!(!before.holds(&main, Access::Work));
    let after = before.with_checkouts();
    assert!(after.holds(&main, Access::Work));
    assert!(after.holds(&real(&gone).unwrap(), Access::Read));
    assert!(!after.holds(tmp.path(), Access::Work));
    // A removed folder with another name opens nothing.
    let other = reach(&main.join("elsewhere/x")).with_checkouts();
    assert!(!other.holds(&main, Access::Work));
    // A worktree still there is its own folder, not its checkout.
    let live = reach(&main.join("src")).with_checkouts();
    assert!(live.work.is_empty());
}

#[test]
fn the_root_folder_is_never_a_reach() {
    let reach = Reached {
        work: vec![PathBuf::from("/")],
        ..Reached::default()
    };
    assert!(!reach.holds(Path::new("/etc/passwd"), Access::Read));
}
