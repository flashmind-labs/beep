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
    // This computer's own node runs it at any version; other nodes after 0.7.10.
    let runs = |host: &AgentHost| match host.machine_name.as_str() {
        "this-box" => true,
        "old-box" => newer_than(Some("0.7.10"), &BEFORE_ASSISTANT),
        _ => newer_than(Some("0.7.11"), &BEFORE_ASSISTANT),
    };

    let ready = for_assistant(
        vec![host("old-box"), host("new-box"), host("this-box")],
        runs,
    )
    .unwrap();
    assert_eq!(ready, vec![host("new-box"), host("this-box")]);

    let refused = for_assistant(vec![host("old-box")], runs).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "Update Beep on old-box to use the assistant."
    );
}

#[test]
fn the_assistant_prefers_this_computer_when_it_offers_the_harness() {
    let hosts = [host("build-box"), host("this-box")];

    let here = this_computer(&hosts, "this-box", Some("claude"));
    assert_eq!(here, Some(host("this-box")));
    assert_eq!(
        this_computer(&hosts, "this-box", None),
        Some(host("this-box"))
    );
    assert_eq!(this_computer(&hosts, "this-box", Some("codex")), None);
    assert_eq!(this_computer(&hosts[..1], "this-box", Some("claude")), None);
}
