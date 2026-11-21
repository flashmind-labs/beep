use super::*;

fn host(name: &str) -> AgentHost {
    AgentHost {
        machine_id: name.to_owned(),
        machine_name: name.to_owned(),
        agents: vec!["claude".to_owned()],
        workspace: None,
        home: None,
        capabilities: Vec::new(),
        hermes: false,
    }
}

#[test]
fn assistant_start_skips_old_nodes() {
    // This computer's own node runs it at any version; other nodes after 0.7.12.
    let runs = |host: &AgentHost| match host.machine_name.as_str() {
        "this-box" => true,
        "old-box" => newer_than(Some("0.7.12"), &BEFORE_ASSISTANT),
        _ => newer_than(Some("0.7.13"), &BEFORE_ASSISTANT),
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

#[test]
fn hermes_starts_only_as_the_assistant_on_a_machine_that_offers_it() {
    let hermes = Some(HERMES_GATEWAY);
    let offering = AgentHost {
        agents: Vec::new(),
        hermes: true,
        ..host("gpu-box")
    };

    assert_eq!(
        start_agent(&offering, hermes, true).unwrap(),
        HERMES_GATEWAY
    );
    assert_eq!(
        start_agent(&offering, hermes, false)
            .unwrap_err()
            .to_string(),
        "Hermes runs only as the Home assistant."
    );
    assert!(
        start_agent(&host("build-box"), hermes, true)
            .unwrap_err()
            .to_string()
            .ends_with("build-box does not offer Hermes.")
    );
    assert_eq!(
        start_agent(&host("build-box"), None, false).unwrap(),
        "claude"
    );
    assert!(start_agent(&offering, None, true).is_err());
}
