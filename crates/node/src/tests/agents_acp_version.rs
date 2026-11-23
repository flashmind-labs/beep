use super::*;
use agent_client_protocol::JsonRpcMessage;

#[test]
fn v1_is_default_with_an_explicit_v2_opt_in_and_invalid_versions_are_refused() {
    assert_eq!(Protocol::default(), Protocol::V1);
    assert_eq!(Protocol::from_env(&BTreeMap::new()).unwrap(), Protocol::V1);
    assert_eq!(
        Protocol::from_env(&BTreeMap::from([("BEEP_ACP_VERSION".into(), "1".into())])).unwrap(),
        Protocol::V1
    );
    assert_eq!(
        Protocol::from_env(&BTreeMap::from([("BEEP_ACP_VERSION".into(), "2".into())])).unwrap(),
        Protocol::V2
    );
    assert!(
        Protocol::from_env(&BTreeMap::from([("BEEP_ACP_VERSION".into(), "3".into())])).is_err()
    );
}

#[test]
fn v2_initialization_retains_v1_capabilities_for_negotiated_fallback() {
    let request = Protocol::V2
        .request(super::super::initialize(true).to_untyped_message().unwrap())
        .unwrap();
    assert_eq!(request.params()["protocolVersion"], 2);
    let legacy = super::super::initialize(true).to_untyped_message().unwrap();
    assert_eq!(
        request.params()["clientCapabilities"],
        legacy.params()["clientCapabilities"]
    );
    assert_eq!(request.params()["clientInfo"], request.params()["info"]);
    assert_eq!(request.params()["info"]["name"], "beep-node");
    assert!(request.params()["capabilities"]["_meta"]["jetbrains"]["air"].is_object());
}

#[test]
fn v2_negotiates_down_to_v1_and_rejects_unsupported_versions() {
    let lower = Protocol::V2
        .initialized(json!({"protocolVersion": 1, "agentCapabilities": {"loadSession": true}}))
        .unwrap();
    assert_eq!(lower.protocol, Protocol::V1);
    assert_eq!(lower.resume, Some(ResumeMethod::Load));
    for version in [0, 3] {
        assert!(
            Protocol::V2
                .initialized(json!({"protocolVersion": version}))
                .is_err()
        );
    }
    assert!(
        Protocol::V1
            .initialized(json!({"protocolVersion": 2}))
            .is_err()
    );
}

#[test]
fn v2_requires_sessions_and_uses_resume_for_replay() {
    assert!(Protocol::V2.initialized(json!({"protocolVersion":2, "info":{"name":"test","version":"1"}, "capabilities":{}})).is_err());
    let initialized = Protocol::V2.initialized(json!({"protocolVersion":2, "info":{"name":"test","version":"1"}, "capabilities":{"session":{"prompt":{"image":{}}}}})).unwrap();
    assert!(initialized.images);
    assert!(!initialized.steering);
    let request = Protocol::V2
        .request(
            UntypedMessage::new(
                "session/load",
                json!({"sessionId":"s", "cwd":"/srv/project", "mcpServers":[]}),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(request.method(), "session/resume");
    assert_eq!(request.params()["replayFrom"], json!({"type":"start"}));
}

#[test]
fn v2_config_options_supply_models_effort_and_modes() {
    let answer = json!({"configOptions":[
        {"configId":"model-picker", "type":"select", "category":"model", "name":"Model", "currentValue":"test", "options":[{"value":"test","name":"Test"}]},
        {"configId":"effort", "type":"select", "category":"thought_level", "name":"Effort", "currentValue":"high", "options":[]},
        {"configId":"permission-mode", "type":"select", "category":"mode", "name":"Mode", "currentValue":"plan", "options":[{"value":"plan","name":"Plan"}]}
    ]});
    let offered = Protocol::V2.offers(&answer);
    assert_eq!(offered.current.as_deref(), Some("test"));
    assert_eq!(offered.thinking.unwrap().to_string(), "effort");
    assert_eq!(offered.mode_config.unwrap().to_string(), "permission-mode");
    assert_eq!(offered.modes.unwrap().current, "plan");
    let request = Protocol::V2
        .request(
            UntypedMessage::new(
                "session/set_config_option",
                json!({"sessionId":"s","configId":"model-picker","value":"test"}),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(request.params()["type"], "id");
}

#[test]
fn v2_session_requests_tag_v1_stdio_mcp_servers() {
    let server = v1::McpServer::Stdio(
        v1::McpServerStdio::new("beep", "/srv/bin/beep-node")
            .args(vec!["mcp".to_owned()])
            .env(vec![v1::EnvVariable::new("BEEP_MCP_TOKEN", "t")]),
    );
    let new = session::new_session(
        std::path::Path::new("/srv/project"),
        vec![server.clone()],
        Some(SystemPromptFormat::Acp.append("brief")),
        true,
    )
    .unwrap();
    let load = session::resume_session(
        super::ResumeMethod::Load,
        v1::SessionId::new("s"),
        std::path::Path::new("/srv/project"),
        vec![server],
        Some(SystemPromptFormat::Acp.append("brief")),
        true,
    )
    .unwrap();

    for request in [new, load] {
        let request = Protocol::V2.request(request).unwrap();
        assert_eq!(request.params()["mcpServers"][0]["type"], "stdio");
    }
}
