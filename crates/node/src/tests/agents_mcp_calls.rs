use super::*;
use serde_json::json;

#[test]
fn calls_are_found_however_the_agent_names_them() {
    assert_eq!(Tool::of_name("mcp__beep__remote_run"), Some(Tool::RunOn));
    assert_eq!(Tool::of_name("mcp__beeper__remote_run"), None);
    assert_eq!(Tool::of_name("mcp__other__remote_run"), None);
    let meta = json!({"claudeCode": {"toolName": "mcp__beep__send_files"}});
    assert_eq!(
        Tool::of_call("Other", meta.as_object(), None),
        Some(Tool::SendFiles)
    );
    assert_eq!(
        Tool::of_call("mcp__beep__list_machines", None, None),
        Some(Tool::ListMachines)
    );
    assert_eq!(
        Tool::of_call("Tool: beep/job_status", None, None),
        Some(Tool::JobStatus)
    );
    assert_eq!(
        Tool::of_call("cancel (beep MCP Server)", None, None),
        Some(Tool::Cancel)
    );
    let codex = json!({"server": "beep", "tool": "ask_user", "arguments": {"question": "Go?"}});
    assert_eq!(
        Tool::of_call("Working", None, Some(&codex)),
        Some(Tool::AskUser)
    );
    assert_eq!(Tool::of_call("Run cargo test", None, None), None);
    assert_eq!(Tool::of_call("beep/unknown", None, None), None);
    assert_eq!(Tool::of_name("mcp__beep__remember"), None);
}
