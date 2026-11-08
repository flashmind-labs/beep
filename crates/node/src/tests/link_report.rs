use super::*;
use std::{fs, os::unix::fs::PermissionsExt};

#[test]
fn metrics_report_keeps_only_snapshots_in_the_requested_period() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("metrics.log");
    fs::write(
        &path,
        "# snapshot unix_seconds=10\na 1\n# end snapshot\n# snapshot unix_seconds=20\na 2\n# end snapshot\n# snapshot unix_seconds=30\na 3\n",
    )
    .expect("metrics");
    let text = recent_metrics(&path, 15, 1024);
    assert!(!text.contains("a 1"));
    assert!(text.contains("# snapshot unix_seconds=20\na 2"));
    assert!(!text.contains("a 3"));
    assert!(recent_metrics(&path, 0, 0).is_empty());
    fs::write(
        dir.path().join("metrics.2026-01-01.log"),
        "# snapshot unix_seconds=5\na 0\n# end snapshot\n",
    )
    .expect("rotated metrics");
    assert!(recent_metrics(&path, 0, 1024).contains("a 0"));
}

#[test]
fn a_program_outside_path_is_listed_with_its_folder() {
    let home = tempfile::tempdir().expect("a temp folder");
    let bin = home.path().join(".local/bin");
    fs::create_dir_all(&bin).expect("folders");
    let claude = bin.join("claude");
    fs::write(&claude, "#!/bin/sh\n").expect("a program");
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).expect("executable");
    let empty = tempfile::tempdir().expect("a temp folder");
    let text = programs(Some(empty.path().into()), Some(home.path()));
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with("claude "))
        .expect("a claude line");
    assert!(
        line.ends_with(&format!("{} (not on PATH)", claude.display())),
        "{line}"
    );
    assert!(text.contains("codex-acp"), "{text}");
}
