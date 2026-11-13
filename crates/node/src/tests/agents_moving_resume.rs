use super::{*, super::folder::Workspace};

#[test]
fn a_session_started_in_the_main_checkout_resumes_in_its_moved_worktree() {
    let folders = Folders {
        old: PathBuf::from("/srv/source/project/.worktrees/fix/src"),
        new: PathBuf::from("/srv/target/project/.worktrees/fix/src"),
        workspace: Some(Workspace {
            checkout: PathBuf::from("/srv/source/project"),
            old: PathBuf::from("/srv/source/project/.worktrees/fix"),
            new: PathBuf::from("/srv/target/project/.worktrees/fix"),
        }),
    };
    let input = concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"cwd\":\"/srv/source/project\",\"runtime_workspace_roots\":[\"/srv/source/project\"]}}\n",
        "{\"type\":\"turn_context\",\"payload\":{\"cwd\":\"/srv/source/project/.worktrees/fix/src\"}}\n",
    );
    let stored = rewrite(
        Harness::Codex,
        input.as_bytes(),
        "session",
        "rollout-2026-11-04T00-00-00-session.jsonl",
        &folders,
    );
    let text = String::from_utf8(stored.bytes).unwrap();
    assert!(!text.contains("/srv/source/"));
    assert!(text.contains(r#""cwd":"/srv/target/project/.worktrees/fix""#));
    assert!(text.contains(r#""cwd":"/srv/target/project/.worktrees/fix/src""#));
    assert!(text.contains(r#""runtime_workspace_roots":["/srv/target/project/.worktrees/fix"]"#));
    assert!(!text.contains(".worktrees/fix/.worktrees/fix"));
}
