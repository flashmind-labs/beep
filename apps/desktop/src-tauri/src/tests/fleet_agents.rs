use super::*;

fn host(name: &str) -> AgentHost {
    AgentHost {
        machine_id: name.to_owned(),
        machine_name: name.to_owned(),
        agents: vec!["claude".to_owned()],
        workspace: None,
        home: None,
        capabilities: Vec::new(),
    }
}

#[test]
fn assistant_start_skips_old_nodes() {
    let version = |host: &AgentHost| match host.machine_name.as_str() {
        "old-box" => Some("0.7.7".to_owned()),
        _ => Some("0.7.8".to_owned()),
    };

    let ready = for_assistant(vec![host("old-box"), host("new-box")], version).unwrap();
    assert_eq!(ready, vec![host("new-box")]);

    let refused = for_assistant(vec![host("old-box")], version).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "Update Beep on old-box to use the assistant."
    );
}
