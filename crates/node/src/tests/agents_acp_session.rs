use super::*;

#[test]
fn a_model_config_option_wins_over_session_models() {
    let answer = serde_json::json!({
        "sessionId": "s1",
        "configOptions": [
            {"id": "mode", "name": "Mode", "category": "mode", "type": "select",
             "currentValue": "ask", "options": [{"value": "ask", "name": "Ask"}]},
            {"id": "model", "name": "Model", "category": "model", "type": "select",
             "currentValue": "gpt-test", "options": [
                {"group": "openai", "name": "OpenAI", "options": [
                    {"value": "gpt-test", "name": "GPT Test"},
                    {"value": "gpt-mini", "name": "GPT Mini", "description": "Faster"}]}]},
        ],
        "models": {"availableModels": [{"modelId": "other"}], "currentModelId": "other"},
    });
    let offered = offered_models(&answer);
    assert_eq!(offered.current.as_deref(), Some("gpt-test"));
    let ids: Vec<&str> = offered.models.iter().map(|m| m.id.as_ref()).collect();
    assert_eq!(ids, ["gpt-test", "gpt-mini"]);
    assert_eq!(offered.models[1].description.as_deref(), Some("Faster"));
    let switch = offered.switch.expect("a switch");
    assert_eq!(
        switch,
        ModelSwitch::ConfigOption(SessionConfigId::new("model"))
    );
    let request = switch_model(&SessionId::new("s1"), &switch, "gpt-mini").unwrap();
    assert_eq!(request.method(), "session/set_config_option");
    assert_eq!(request.params()["configId"], "model");
    assert_eq!(request.params()["value"], "gpt-mini");
}

/// Codex's answer to `session/new` and `session/load`, trimmed: its
/// model selector sits among categories the crate does not name.
fn codex_answer() -> Value {
    serde_json::json!({
        "sessionId": "s1",
        "models": {
            "availableModels": [
                {"modelId": "gpt-test[low]", "name": "Test (low)"},
                {"modelId": "gpt-test[high]", "name": "Test (high)"},
            ],
            "currentModelId": "gpt-test[high]",
        },
        "modes": {"currentModeId": "agent", "availableModes": [{"id": "agent", "name": "Agent"}]},
        "configOptions": [
            {"id": "mode", "name": "Mode", "category": "mode", "type": "select",
             "currentValue": "agent", "options": [{"value": "agent", "name": "Agent"}]},
            {"id": "collaboration_mode", "name": "Collaboration mode",
             "category": "collaboration_mode", "type": "select", "currentValue": "default",
             "options": [{"value": "default", "name": "Default"}]},
            {"id": "model", "name": "Model", "category": "model", "type": "select",
             "currentValue": "gpt-test", "options": [
                {"value": "gpt-test", "name": "Test"},
                {"value": "gpt-mini", "name": "Mini", "description": "Faster"}]},
            {"id": "reasoning_effort", "name": "Reasoning effort", "category": "thought_level",
             "type": "select", "currentValue": "high", "options": [
                {"value": "low", "name": "Low"}, {"value": "high", "name": "High"}]},
            {"id": "fast-mode", "name": "Fast mode", "category": "model_config",
             "type": "select", "currentValue": "off", "options": [{"value": "off", "name": "Off"}]},
        ],
    })
}

#[test]
fn codex_offers_its_model_selector() {
    let (_, offered) = session_created(codex_answer()).unwrap();
    assert_eq!(
        offered.switch,
        Some(ModelSwitch::ConfigOption(SessionConfigId::new("model")))
    );
    assert_eq!(offered.current.as_deref(), Some("gpt-test"));
    assert_eq!(offered.models.len(), 2);
    assert_eq!(
        offered.thinking,
        Some(SessionConfigId::new("reasoning_effort"))
    );
    // `session/load` answers the same without a `sessionId`.
    let mut loaded = codex_answer();
    loaded.as_object_mut().unwrap().remove("sessionId");
    assert_eq!(offers(&loaded), offered);
}

#[test]
fn antigravity_offers_per_model_thinking_levels() {
    let answer = serde_json::json!({
        "sessionId": "s1",
        "models": {
            "availableModels": [
                {"modelId": "gemini-4-neon-high", "name": "Gemini 4 Neon (High)"},
                {"modelId": "gemini-4-neon-xhigh", "name": "Gemini 4 Neon (XHigh)"},
                {"modelId": "gemini-4-neon-medium", "name": "Gemini 4 Neon (Medium)"},
                {"modelId": "gemini-4-neon-low", "name": "Gemini 4 Neon (Low)"},
                {"modelId": "gemini-4-carbon-medium", "name": "Gemini 4 Carbon (Medium)"},
                {"modelId": "gemini-4-carbon-high", "name": "Gemini 4 Carbon (High)"},
                {"modelId": "gemini-4-carbon-low", "name": "Gemini 4 Carbon (Low)"},
                {"modelId": "gpt-oss-120b-medium", "name": "GPT-OSS 120B (Medium)"},
            ],
            "currentModelId": "gemini-4-neon-high",
        },
        "configOptions": [
            {"id": "model", "name": "Model", "category": "model", "type": "select",
             "currentValue": "gemini-4-neon", "options": [
                {"value": "gemini-4-neon", "name": "Gemini 4 Neon"},
                {"value": "gemini-4-carbon", "name": "Gemini 4 Carbon"},
                {"value": "gpt-oss-120b-medium", "name": "GPT-OSS 120B (Medium)"}]},
            {"id": "thought_level", "name": "Thinking Level", "category": "thought_level",
             "type": "select", "currentValue": "high", "options": [
                {"value": "low", "name": "Low"},
                {"value": "medium", "name": "Medium"},
                {"value": "high", "name": "High"},
                {"value": "xhigh", "name": "Extra"}]},
        ],
    });
    let (_, offered) = session_created(answer).unwrap();
    assert_eq!(
        offered.thinking,
        Some(SessionConfigId::new("thought_level"))
    );
    assert_eq!(
        offered.models[0].efforts,
        vec![Effort::Low, Effort::Medium, Effort::High, Effort::Xhigh]
    );
    assert_eq!(
        offered.models[1].efforts,
        vec![Effort::Low, Effort::Medium, Effort::High]
    );
    assert!(offered.models[2].efforts.is_empty());
}

#[test]
fn an_agent_without_models_offers_none() {
    let offered = offered_models(&serde_json::json!({"sessionId": "s1"}));
    assert_eq!(offered, Offered::default());
    // A broken `models` is ignored rather than failing the session.
    let broken = serde_json::json!({"sessionId": "s1", "models": {"availableModels": 3}});
    assert_eq!(offered_models(&broken), Offered::default());
    assert!(session_created(broken).is_ok());
}

/// Stop ends only the main turn: every way a session starts tells Claude
/// Code's adapter that the client stops background tasks one by one, so
/// its interrupt spares background subagents (#565).
#[test]
fn every_session_start_lets_a_stop_spare_background_subagents() {
    let cwd = std::path::Path::new("/srv/project");
    let requests = [
        new_session(cwd, Vec::new(), None, true).unwrap(),
        resume_session(
            ResumeMethod::Load,
            SessionId::new("s1"),
            cwd,
            Vec::new(),
            None,
            true,
        )
        .unwrap(),
        resume_session(
            ResumeMethod::Resume,
            SessionId::new("s1"),
            cwd,
            Vec::new(),
            None,
            true,
        )
        .unwrap(),
    ];

    for request in requests {
        assert_eq!(
            request.params()["_meta"]["claudeCode"]["options"]["perTaskStopAffordance"],
            true,
            "{}",
            request.method()
        );
        assert!(request.params()["_meta"].get("flsh").is_none());
    }
}

/// The home assistant starts threads, which the user sees, so every way its
/// session starts switches off the harness's own subagents (#1127).
#[test]
fn the_assistant_starts_without_its_harness_subagents() {
    let cwd = std::path::Path::new("/srv/project");
    let requests = [
        new_session(cwd, Vec::new(), None, false).unwrap(),
        resume_session(
            ResumeMethod::Load,
            SessionId::new("s1"),
            cwd,
            Vec::new(),
            None,
            false,
        )
        .unwrap(),
        resume_session(
            ResumeMethod::Resume,
            SessionId::new("s1"),
            cwd,
            Vec::new(),
            None,
            false,
        )
        .unwrap(),
    ];

    for request in requests {
        let meta = &request.params()["_meta"];
        assert_eq!(
            meta["claudeCode"]["options"]["disallowedTools"],
            serde_json::json!(["Agent", "Task"])
        );
        assert_eq!(meta["claudeCode"]["options"]["perTaskStopAffordance"], true);
        assert_eq!(meta["flsh"]["subagents"], false, "{}", request.method());
    }
}
