use super::*;
use std::process::Command;

/// Runs `script` as Git's sh runs it on Windows, with the variable PowerShell sets.
fn as_git_sh_runs(script: &str) -> (Option<i32>, String) {
    let wrapped = posix_on_windows(script);
    let encoded = wrapped
        .split("FromBase64String('")
        .nth(1)
        .and_then(|rest| rest.split('\'').next())
        .expect("the script travels as base64");
    let decoded = String::from_utf8(STANDARD.decode(encoded).expect("base64")).expect("utf-8");
    let out = Command::new("sh")
        .arg("-c")
        .arg(SH_LINE)
        .env("BEEP_SH", decoded)
        .output()
        .expect("sh runs");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn a_windows_machine_runs_the_script_whole_in_git_sh() {
    let script = "x='a  b'; for w in $x; do echo \"[$w]\"; done; echo \"$x\"; \
                      case $- in *f*) echo noglob;; esac; echo 'it''s'; echo oops >&2; exit 7";
    let (code, out) = as_git_sh_runs(script);
    assert_eq!(code, Some(7));
    // Splitting and globbing are back for the script itself, and stderr is merged.
    assert_eq!(out, "[a]\n[b]\na  b\nits\noops\n");
    let wrapped = posix_on_windows(script);
    assert!(
        !wrapped.contains("$x"),
        "the script never shows in the line"
    );
    assert!(wrapped.contains(&format!("-c '{SH_LINE}'")));
    assert!(!SH_LINE.contains('"') && !SH_LINE.contains('\''));
}

#[test]
fn only_windows_gets_the_wrapper() {
    assert_eq!(for_machine("echo hi", false), "echo hi");
    let windows = for_machine("echo hi", true);
    assert!(windows.contains("Get-Command git") && windows.contains("bin\\sh.exe"));
    assert!(windows.contains(NO_GIT_SH_TEXT));
    assert!(is_windows("windows") && !is_windows("macos"));
}

#[test]
fn output_keeps_its_start_and_end_on_character_boundaries() {
    let mut clip = Clip::default();
    clip.push(&"a".repeat(HEAD - 1));
    clip.push("éé");
    clip.push(&"b".repeat(TAIL * 2));
    clip.push("end");
    let kept = clip.finish();
    assert!(kept.starts_with("aaa"));
    assert!(kept.ends_with("bbbend"));
    assert!(kept.len() <= HEAD + 1 + TAIL + 1);
}

#[test]
fn a_short_head_takes_nothing_once_the_tail_has_content() {
    let mut clip = Clip::default();
    clip.push(&"a".repeat(HEAD - 1));
    clip.push("éé");
    clip.push("z");
    let kept = clip.finish();
    assert!(kept.ends_with("ééz"));
}
