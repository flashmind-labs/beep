use super::*;

#[test]
fn an_api_key_is_not_taken_as_a_claude_token() {
    assert!(refusal(AgentAccount::Claude, "sk-ant-api03-abcdef").is_some());
    assert_eq!(refusal(AgentAccount::Claude, "sk-ant-oat01-abcdef"), None);
    assert_eq!(refusal(AgentAccount::Codex, "sk-proj-abcdef"), None);
}
