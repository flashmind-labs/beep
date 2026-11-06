//! The command line on its own, with no node running.

use std::process::Command;
use tempfile::TempDir;

fn beep_node(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_beep-node"))
        .args(args)
        .output()
        .expect("beep-node runs");
    assert!(
        out.status.success(),
        "beep-node {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("text")
}

#[test]
fn help_lists_the_commands_in_groups() {
    let help = beep_node(&["--help"]);
    for heading in ["Mesh", "Agents", "Service", "Diagnostics", "Setup"] {
        assert!(
            help.lines().any(|line| line == heading),
            "no {heading} group in:\n{help}"
        );
    }
    for listed in [
        "mesh join",
        "status",
        "[aliases: s, st]",
        "beep-node help <command>",
    ] {
        assert!(help.contains(listed), "{listed} missing from:\n{help}");
    }
}

#[test]
fn status_says_when_no_node_runs_and_how_to_start_one() {
    let dir = TempDir::new().expect("a temp folder");
    let dir = dir.path().to_str().expect("a utf-8 path");
    let text = beep_node(&["status", "--dir", dir]);
    assert!(text.contains("not running"), "{text}");
    assert!(text.contains("beep-node run"), "{text}");
    assert_eq!(beep_node(&["st", "--dir", dir]), text);
}
