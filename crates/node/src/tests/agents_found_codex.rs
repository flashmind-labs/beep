use super::*;
use std::fs;

const ID: &str = "01a199e3-03f2-7820-a29e-067ae8c4ec85";

fn write(root: &Path, name: &str, lines: &[String]) -> PathBuf {
    let dir = root.join("sessions/2026/11/02");
    fs::create_dir_all(&dir).expect("folders");
    let path = dir.join(name);
    fs::write(&path, lines.join("\n") + "\n").expect("a rollout");
    path
}

fn meta(id: &str, source: &str) -> String {
    format!(
        r#"{{"timestamp":"2026-11-02T01:32:34.000Z","type":"session_meta","payload":{{"id":"{id}","timestamp":"2026-11-02T01:32:34.000Z","cwd":"/home/user/proj","source":{source},"base_instructions":{{"text":"You are Codex."}}}}}}"#
    )
}

#[test]
fn reads_rollouts_and_leaves_out_sub_agents() {
    let root = tempfile::tempdir().expect("a temp folder");
    let user =
        r#"{"type":"event_msg","payload":{"type":"user_message","message":"Add a retry flag"}}"#;
    let context = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>"}]}}"#;
    let path = write(
        root.path(),
        &format!("rollout-2026-11-02T02-32-34-{ID}.jsonl"),
        &[meta(ID, r#""cli""#), context.into(), user.into()],
    );
    write(
        root.path(),
        "rollout-2026-11-02T02-40-00-sub.jsonl",
        &[meta("sub", r#"{"subagent":"review"}"#)],
    );
    write(root.path(), "notes.jsonl", &[meta("x", r#""cli""#)]);
    let found = sessions(root.path(), &Memos::default());
    assert_eq!(found.len(), 1, "{found:?}");
    let one = &found[0];
    assert_eq!(one.agent, Agent::Codex);
    assert_eq!(one.id, ID);
    assert_eq!(one.cwd, Path::new("/home/user/proj"));
    assert!(one.started_at > 0);
    assert_eq!(one.title, None);
    assert_eq!(title(&path).as_deref(), Some("Add a retry flag"));
}

#[test]
fn the_latest_folder_is_a_calls_workdir_after_the_turns_cwd() {
    let root = tempfile::tempdir().expect("a temp folder");
    let turn =
        |cwd: &str| format!(r#"{{"type":"turn_context","payload":{{"cwd":"{cwd}","model":"m"}}}}"#);
    let call = |args: &str| {
        let args = args.replace('"', "\\\"");
        format!(
            r#"{{"type":"response_item","payload":{{"type":"function_call","name":"exec_command","arguments":"{args}","call_id":"c"}}}}"#
        )
    };
    let path = write(
        root.path(),
        "rollout-a.jsonl",
        &[
            meta(ID, r#""cli""#),
            turn("/srv/project"),
            call(r#"{"cmd":"ls","workdir":"/srv/project/.worktrees/fix"}"#),
        ],
    );
    assert_eq!(
        latest_folder(&path),
        Some(PathBuf::from("/srv/project/.worktrees/fix"))
    );
    // A later turn's cwd wins over an earlier call's workdir.
    let path = write(
        root.path(),
        "rollout-b.jsonl",
        &[
            turn("/srv/project"),
            call(r#"{"cmd":"ls","workdir":"/srv/project/.worktrees/fix"}"#),
            turn("/srv/project/.worktrees/other"),
        ],
    );
    assert_eq!(
        latest_folder(&path),
        Some(PathBuf::from("/srv/project/.worktrees/other"))
    );
    let none = write(root.path(), "rollout-c.jsonl", &[meta(ID, r#""cli""#)]);
    assert_eq!(latest_folder(&none), None);
}

#[test]
fn a_newer_user_message_reads_too() {
    let line = r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"i","content":[{"type":"text","text":"Fix the parser","text_elements":[]}]}}}"#;
    assert_eq!(prompt(line).as_deref(), Some("Fix the parser"));
    assert_eq!(header("{\"type\":\"turn_context\"}"), None);
}

#[test]
fn a_workdir_survives_a_new_turn_in_the_same_launch_folder() {
    let root = tempfile::tempdir().expect("a temp folder");
    let turn =
        serde_json::json!({"type": "turn_context", "payload": {"cwd": "/srv/project"}}).to_string();
    let call = serde_json::json!({"type": "response_item", "payload": {
        "type": "function_call", "name": "exec_command",
        "arguments": serde_json::json!({"cmd": "git diff", "workdir": "/srv/project/.worktrees/fix"}).to_string()
    }}).to_string();
    let path = write(root.path(), "wrapped.jsonl", &[turn.clone(), call, turn]);
    assert_eq!(
        latest_folder(&path),
        Some(PathBuf::from("/srv/project/.worktrees/fix"))
    );
}

#[test]
fn image_only_rollouts_use_the_next_text_prompt_as_title() {
    let root = tempfile::tempdir().unwrap();
    let image = r#"{"type":"event_msg","payload":{"type":"user_message","message":"[image: image-0123456789abcdef]"}}"#;
    let path = write(
        root.path(),
        "rollout-images.jsonl",
        &[meta(ID, r#""cli""#), image.into()],
    );
    assert_eq!(title(&path), None);
    let words =
        r#"{"type":"event_msg","payload":{"type":"user_message","message":"Fix the overlap"}}"#;
    fs::write(
        &path,
        [meta(ID, r#""cli""#), image.into(), words.into()].join("\n"),
    )
    .unwrap();
    assert_eq!(title(&path).as_deref(), Some("Fix the overlap"));
}
