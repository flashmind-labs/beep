use super::*;

const ID: &str = "0b1c2d3e-0000-4000-8000-0123456789ab";

fn move_with(harness: Harness, transcript: &str, file: &str) -> Stored {
    rewrite(
        harness,
        transcript.as_bytes(),
        ID,
        file,
        Path::new("/srv/laptop/app"),
        Path::new("/work/app"),
    )
}

fn text(stored: &Stored) -> &str {
    std::str::from_utf8(&stored.bytes).expect("utf8")
}

#[test]
fn claude_changes_cwd_on_each_line_and_files_under_the_new_slug() {
    let input = concat!(
        "{\"type\":\"user\",\"cwd\":\"/srv/laptop/app\",\"message\":{\"content\":\"cwd is \\\"/srv/laptop/app\\\"\"}}\n",
        "{\"cwd\":\"/srv/laptop/app/sub\",\"type\":\"assistant\", \"n\": 1.50}\n",
        "{\"type\":\"summary\"}\n",
        "{\"cwd\":\"/elsewhere\"}\n",
        "{\"cwd\":\"/srv/laptop/app\",\"type\":\"user\"}",
    );
    let out = move_with(Harness::Claude, input, "x.jsonl");
    let want = concat!(
        "{\"type\":\"user\",\"cwd\":\"/work/app\",\"message\":{\"content\":\"cwd is \\\"/srv/laptop/app\\\"\"}}\n",
        "{\"cwd\":\"/work/app/sub\",\"type\":\"assistant\", \"n\": 1.50}\n",
        "{\"type\":\"summary\"}\n",
        "{\"cwd\":\"/elsewhere\"}\n",
        "{\"cwd\":\"/work/app\",\"type\":\"user\"}",
    );
    assert_eq!(text(&out), want);
    assert_eq!(out.path, PathBuf::from(format!("-work-app/{ID}.jsonl")));
}

#[test]
fn claude_keeps_a_line_it_cannot_read() {
    let input = "not json \"cwd\":\"/srv/laptop/app\"\n{\"cwd\":\n";
    let out = move_with(Harness::Claude, input, "x.jsonl");
    assert_eq!(text(&out), input);
}

#[test]
fn codex_changes_meta_roots_and_turn_contexts_only() {
    let file = format!("rollout-2026-11-04T09-30-00-{ID}.jsonl");
    let input = concat!(
        "{\"timestamp\":\"t\",\"type\":\"session_meta\",\"payload\":{\"id\":\"x\",\"cwd\":\"/srv/laptop/app\",\"runtime_workspace_roots\":[\"/srv/laptop/app\",\"/other\"],\"note\":\"/srv/laptop/app\"}}\n",
        "{\"type\":\"turn_context\",\"payload\":{\"cwd\":\"file:///srv/laptop/app/my%20dir\",\"model\":\"m\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"cwd\":\"/srv/laptop/app\",\"text\":\"/srv/laptop/app\"}}\n",
    );
    let out = move_with(Harness::Codex, input, &file);
    let want = concat!(
        "{\"timestamp\":\"t\",\"type\":\"session_meta\",\"payload\":{\"id\":\"x\",\"cwd\":\"/work/app\",\"runtime_workspace_roots\":[\"/work/app\",\"/other\"],\"note\":\"/srv/laptop/app\"}}\n",
        "{\"type\":\"turn_context\",\"payload\":{\"cwd\":\"file:///work/app/my%20dir\",\"model\":\"m\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"cwd\":\"/srv/laptop/app\",\"text\":\"/srv/laptop/app\"}}\n",
    );
    assert_eq!(text(&out), want);
    assert_eq!(out.path, PathBuf::from("sessions/2026/11/04").join(&file));
}

#[test]
fn pi_changes_the_header_only_and_files_under_the_folder_name() {
    let file = format!("2026-11-04T09-30-00-000Z_{ID}.jsonl");
    let input = concat!(
        "{\"type\":\"session\",\"version\":3,\"id\":\"x\",\"cwd\":\"/srv/laptop/app\"}\n",
        "{\"type\":\"message\",\"cwd\":\"/srv/laptop/app\"}\n",
    );
    let out = move_with(Harness::Pi, input, &file);
    let want = concat!(
        "{\"type\":\"session\",\"version\":3,\"id\":\"x\",\"cwd\":\"/work/app\"}\n",
        "{\"type\":\"message\",\"cwd\":\"/srv/laptop/app\"}\n",
    );
    assert_eq!(text(&out), want);
    assert_eq!(out.path, PathBuf::from("--work-app--").join(&file));
}

#[test]
fn a_folder_that_only_shares_a_prefix_is_not_inside() {
    let input = "{\"cwd\":\"/srv/laptop/app2\"}\n";
    let out = move_with(Harness::Claude, input, "x.jsonl");
    assert_eq!(text(&out), input);
}

#[test]
fn only_stores_in_files_have_a_harness() {
    assert_eq!(Harness::of(Agent::Pi), Some(Harness::Pi));
    assert_eq!(Harness::of(Agent::OpenCode), None);
}
