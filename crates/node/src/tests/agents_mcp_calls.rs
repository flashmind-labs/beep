use super::*;
use serde_json::json;

#[test]
fn calls_are_found_however_the_agent_names_them() {
    assert_eq!(Tool::of_name("mcp__beep__remote_run"), Some(Tool::RunOn));
    assert_eq!(Tool::of_name("mcp__beeper__remote_run"), None);
    assert_eq!(Tool::of_name("mcp__other__remote_run"), None);
    let meta = json!({"claudeCode": {"toolName": "mcp__beep__send_files"}});
    assert_eq!(
        Tool::of_call(None, "Other", meta.as_object(), None),
        Some(Tool::SendFiles)
    );
    assert_eq!(
        Tool::of_call(Some("mcp__beep__remote_run"), "Other", None, None),
        Some(Tool::RunOn)
    );
    assert_eq!(
        Tool::of_call(None, "mcp__beep__list_machines", None, None),
        Some(Tool::ListMachines)
    );
    assert_eq!(
        Tool::of_call(
            None,
            "mcp__beep__start_agent: background, cwd, harness, machine, prompt, title",
            None,
            None,
        ),
        Some(Tool::StartAgent)
    );
    assert_eq!(
        Tool::of_call(None, "Tool: beep/job_status", None, None),
        Some(Tool::JobStatus)
    );
    assert_eq!(
        Tool::of_call(None, "Tool: beep/job_status: id", None, None),
        Some(Tool::JobStatus)
    );
    let codex = json!({"server": "beep", "tool": "ask_user", "arguments": {"question": "Go?"}});
    assert_eq!(
        Tool::of_call(None, "Working", None, Some(&codex)),
        Some(Tool::AskUser)
    );
    assert_eq!(Tool::of_call(None, "Run cargo test", None, None), None);
    assert_eq!(Tool::of_call(None, "beep/unknown", None, None), None);
    assert_eq!(Tool::of_name("mcp__beep__remember"), None);
}

#[test]
fn device_calls_name_the_device_and_its_machine() {
    let title = |name: &str, args: Value| {
        Tool::of_name(&format!("mcp__beep__{name}"))
            .unwrap()
            .title(Some(&args))
    };
    assert_eq!(title("device_list", json!({})), "Beep: List devices");
    assert_eq!(
        title("device_boot", json!({"machine": "Mac Box", "device": "iPhone 17"})),
        "Beep: Boot iPhone 17 on mac-box"
    );
    assert_eq!(
        title("device_tap", json!({"machine": "mac-box", "device": "iPhone 17", "x": 120, "y": 40.5})),
        "Beep: Tap iPhone 17 on mac-box: 120, 40.5"
    );
    assert_eq!(
        title("device_type", json!({"machine": "mac-box", "device": "iPhone 17", "text": "hi"})),
        "Beep: Type into iPhone 17 on mac-box: hi"
    );
    assert_eq!(
        title("device_press", json!({"machine": "mac-box", "device": "Pixel 9", "key": "home"})),
        "Beep: Press home on Pixel 9 on mac-box"
    );
    assert_eq!(title("device_ui", json!({})), "Beep: Use a device on a machine");
}

#[test]
fn renamed_tools_read_by_either_name() {
    assert_eq!(Tool::of_name("mcp__beep__remote_run_status"), Some(Tool::JobStatus));
    assert_eq!(Tool::of_name("mcp__beep__job_status"), Some(Tool::JobStatus));
    assert_eq!(Tool::of_name("mcp__beep__remote_run_cancel"), Some(Tool::Cancel));
    assert_eq!(Tool::of_name("mcp__beep__beep_report"), Some(Tool::FileReport));
}

#[test]
fn grouped_calls_take_the_words_of_their_action() {
    let title = |name: &str, args: Value| {
        Tool::of_name(&format!("mcp__beep__{name}"))
            .unwrap()
            .title(Some(&args))
    };
    assert_eq!(
        title("run", json!({"action": "start", "machine": "build-box", "command": "cargo test"})),
        "Beep: Run on build-box: cargo test"
    );
    assert_eq!(
        title("agent", json!({"action": "start", "harness": "codex", "prompt": "Fix it"})),
        "Beep: Start codex agent: Fix it"
    );
    assert_eq!(
        title("files", json!({"action": "sync", "project": "beep"})),
        "Beep: Sync beep"
    );
    assert_eq!(title("machines", json!({"action": "list"})), "Beep: List machines");
    assert_eq!(
        title("beep", json!({"action": "logs", "machine": "build-box"})),
        "Beep: Read logs on build-box"
    );
    assert_eq!(title("agent", json!({"action": "dance"})), "Beep: Use agent");
    assert_eq!(
        title("device", json!({"action": "boot", "machine": "mac-box", "device": "Pixel 9"})),
        "Beep: Boot Pixel 9 on mac-box"
    );
}

#[test]
fn project_calls_name_their_action() {
    let title = |name: &str, args: Value| Tool::of_name(name).unwrap().title(Some(&args));
    assert_eq!(
        title("mcp__beep__projects_hosts", json!({})),
        "Beep: List Git hosts"
    );
    assert_eq!(
        title(
            "mcp__beep__projects_create",
            json!({"name": "notes", "machines": ["build-box"]})
        ),
        "Beep: Create project: notes"
    );
    assert_eq!(
        title(
            "mcp__beep__projects_clone",
            json!({"url": "https://example.com/org/repo"})
        ),
        "Beep: Clone project: https://example.com/org/repo"
    );
    assert_eq!(
        title("mcp__beep__projects_found", json!({})),
        "Beep: Find projects"
    );
    assert_eq!(
        title("mcp__beep__projects_import", json!({"projects": ["org/repo"]})),
        "Beep: Import projects"
    );
    assert_eq!(
        title("mcp__beep__team_join", json!({"ticket": "beepteam1secret"})),
        "Beep: Join a team"
    );
    assert_eq!(Tool::of_name("mcp__beep__projects"), None);
}
