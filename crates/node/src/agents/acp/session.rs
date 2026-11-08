//! Session startup, capabilities and model configuration.

use agent_client_protocol::{
    UntypedMessage,
    schema::{
        ProtocolVersion,
        v1::{
            AGENT_METHOD_NAMES, ClientCapabilities, ClientSessionCapabilities,
            CompactionCapabilities, ContentBlock, Implementation, InitializeRequest,
            InitializeResponse, LoadSessionRequest, McpServer, NewSessionRequest,
            NewSessionResponse, ResumeSessionRequest, SessionConfigId, SessionConfigKind,
            SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption,
            SessionConfigSelectOptions, SessionId, SessionModeState, SetSessionConfigOptionRequest,
            SystemPrompt, TextContent,
        },
    },
};
use beep_proto::node::agent::{Effort, ModeOption, ModelOption};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::path::Path;

/// What `initialize` sends: this client reads and writes no files and runs
/// no terminals for the agent (the crate's defaults), so it does that work
/// itself. It takes `compaction_update`s, which the transcript shows. With
/// `async_tasks` it also says, in the AIR extension Claude Code's adapter
/// reads, that it takes the commands the agent runs in the background
/// (`async_task_*` updates, see `AsyncTask`) and can stop them.
pub(in crate::agents) fn initialize(async_tasks: bool) -> InitializeRequest {
    let session = ClientSessionCapabilities::new().compaction(CompactionCapabilities::new());
    let mut capabilities = ClientCapabilities::new().session(session);
    if async_tasks {
        let mut meta = Map::new();
        meta.insert(
            "jetbrains".to_owned(),
            json!({"air": {"version": 1, "capabilities": ["asyncTasks"]}}),
        );
        capabilities = capabilities.meta(meta);
    }
    InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(capabilities)
        .client_info(Implementation::new("beep-node", env!("CARGO_PKG_VERSION")))
}

/// How an agent resumes a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::agents) enum ResumeMethod {
    /// `session/resume`, which replays nothing.
    Resume,
    /// `session/load`, which replays the history.
    Load,
}

/// How the agent the `initialize` answer describes resumes a session,
/// preferring `session/load`, which replays the conversation so the
/// session shows its history; `None` when it does neither.
pub(in crate::agents) fn resume_method(initialized: &InitializeResponse) -> Option<ResumeMethod> {
    let capabilities = &initialized.agent_capabilities;
    if capabilities.load_session {
        Some(ResumeMethod::Load)
    } else if capabilities.session_capabilities.resume.is_some() {
        Some(ResumeMethod::Resume)
    } else {
        None
    }
}

/// The most models kept from one agent's list.
pub(in crate::agents) const MAX_MODELS: usize = 500;

/// The most session modes kept from one agent's list.
const MAX_MODES: usize = 32;

/// The id of the mode where an agent plans and changes nothing.
pub(in crate::agents) const PLAN_MODE: &str = "plan";

/// The models an agent offers in its answer to `session/new`, and how it
/// switches between them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(in crate::agents) struct Offered {
    /// The model the session runs on, when it says.
    pub(in crate::agents) current: Option<String>,
    pub(in crate::agents) models: Vec<ModelOption>,
    pub(in crate::agents) switch: Option<ModelSwitch>,
    /// Its config option of category `thought_level`, which sets how hard
    /// it thinks, when it has one.
    pub(in crate::agents) thinking: Option<SessionConfigId>,
    /// The session modes it offers and the one it is in, when it has any.
    pub(in crate::agents) modes: Option<Modes>,
}

/// The session modes an agent offers, in the order it lists them.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::agents) struct Modes {
    pub(in crate::agents) current: String,
    pub(in crate::agents) available: Vec<ModeOption>,
}

/// How an agent switches a session's model.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::agents) enum ModelSwitch {
    /// `session/set_config_option` on its selector of category `model`.
    ConfigOption(SessionConfigId),
    /// `session/set_model`, from ACP's unstable session models, which the
    /// Claude Code, Codex and OpenCode adapters answer.
    SetModel,
}

/// `models` in an answer to `session/new`: ACP's unstable session models.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionModels {
    #[serde(default)]
    available_models: Vec<AcpModel>,
    #[serde(default)]
    current_model_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcpModel {
    model_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

/// Whether the agent the `initialize` answer describes takes a system
/// prompt on `session/new`, `session/load` and `session/resume` (ACP's
/// unstable `sessionCapabilities.systemPrompt`; every such agent appends).
pub(in crate::agents) fn takes_system_prompt(initialized: &InitializeResponse) -> bool {
    initialized
        .agent_capabilities
        .session_capabilities
        .system_prompt
        .is_some()
}

/// `text` appended to the agent's own system prompt.
fn system_prompt(text: &str) -> SystemPrompt {
    SystemPrompt::new(vec![ContentBlock::Text(TextContent::new(text))])
}

/// `session/new` in `cwd`, with `brief` appended to the agent's system
/// prompt, sent untyped so the answer keeps `models`, which the crate's
/// typed answer drops.
pub(in crate::agents) fn new_session(
    cwd: &Path,
    mcp_servers: Vec<McpServer>,
    brief: Option<&str>,
) -> Result<UntypedMessage, agent_client_protocol::Error> {
    UntypedMessage::new(
        AGENT_METHOD_NAMES.session_new,
        NewSessionRequest::new(cwd)
            .mcp_servers(mcp_servers)
            .system_prompt(brief.map(system_prompt)),
    )
}

/// `session/load` or `session/resume` of `session` in `cwd`, with `brief`
/// appended to the agent's system prompt, sent untyped so the answer keeps
/// `models`, as `new_session` does.
pub(in crate::agents) fn resume_session(
    method: ResumeMethod,
    session: SessionId,
    cwd: &Path,
    mcp_servers: Vec<McpServer>,
    brief: Option<&str>,
) -> Result<UntypedMessage, agent_client_protocol::Error> {
    let brief = brief.map(system_prompt);
    match method {
        ResumeMethod::Load => UntypedMessage::new(
            AGENT_METHOD_NAMES.session_load,
            LoadSessionRequest::new(session, cwd)
                .mcp_servers(mcp_servers)
                .system_prompt(brief),
        ),
        ResumeMethod::Resume => UntypedMessage::new(
            AGENT_METHOD_NAMES.session_resume,
            ResumeSessionRequest::new(session, cwd)
                .mcp_servers(mcp_servers)
                .system_prompt(brief),
        ),
    }
}

/// The session an answer to `session/new` made, and the models it offers.
pub(in crate::agents) fn session_created(
    answer: Value,
) -> Result<(NewSessionResponse, Offered), serde_json::Error> {
    let offered = offers(&answer);
    Ok((serde_json::from_value(answer)?, offered))
}

/// The models and thinking option an answer to `session/new`,
/// `session/load` or `session/resume` offers.
pub(in crate::agents) fn offers(answer: &Value) -> Offered {
    let options = config_options(answer);
    Offered {
        thinking: thinking_option(&options),
        modes: session_modes(answer),
        ..models_from_options(answer, options)
    }
}

/// `modes` in an answer; one that does not read is skipped, as a broken
/// `models` is.
fn session_modes(answer: &Value) -> Option<Modes> {
    let modes = SessionModeState::deserialize(answer.get("modes")?).ok()?;
    Some(Modes {
        current: modes.current_mode_id.to_string(),
        available: modes
            .available_modes
            .into_iter()
            .take(MAX_MODES)
            .map(|mode| ModeOption {
                id: mode.id.to_string(),
                name: mode.name,
            })
            .collect(),
    })
}

/// The config options in an answer that offers them, skipping any that do
/// not read. The answer to `session/load` has no `sessionId`, so the
/// options are read on their own rather than as a `NewSessionResponse`.
fn config_options(answer: &Value) -> Vec<SessionConfigOption> {
    let Some(options) = answer.get("configOptions").and_then(Value::as_array) else {
        return Vec::new();
    };
    options
        .iter()
        .filter_map(|option| SessionConfigOption::deserialize(option).ok())
        .collect()
}

/// The selector of category `thought_level` in an answer:
/// Claude Code's `effort`, Codex's `reasoning_effort` and pi's
/// `thought_level`, which take the values `Effort` names.
fn thinking_option(options: &[SessionConfigOption]) -> Option<SessionConfigId> {
    options.iter().find_map(|option| {
        let thinking = matches!(option.kind, SessionConfigKind::Select(_))
            && matches!(
                option.category,
                Some(SessionConfigOptionCategory::ThoughtLevel)
            );
        thinking.then(|| option.id.clone())
    })
}

/// The request that sets how hard `session` thinks, on its option `id`.
pub(in crate::agents) fn set_effort(
    session: &SessionId,
    id: &SessionConfigId,
    effort: Effort,
) -> Result<UntypedMessage, agent_client_protocol::Error> {
    UntypedMessage::new(
        AGENT_METHOD_NAMES.session_set_config_option,
        SetSessionConfigOptionRequest::new(session.clone(), id.clone(), effort.as_ref()),
    )
}

/// The models an answer offers: its config option of
/// category `model` when it has one, else its unstable `models`.
pub(in crate::agents) fn offered_models(answer: &Value) -> Offered {
    models_from_options(answer, config_options(answer))
}

fn models_from_options(answer: &Value, options: Vec<SessionConfigOption>) -> Offered {
    let selector = options.into_iter().find_map(|option| {
        let SessionConfigKind::Select(select) = option.kind else {
            return None;
        };
        matches!(option.category, Some(SessionConfigOptionCategory::Model))
            .then_some((option.id, select))
    });
    if let Some((id, select)) = selector {
        let choices: Vec<SessionConfigSelectOption> = match select.options {
            SessionConfigSelectOptions::Ungrouped(options) => options,
            SessionConfigSelectOptions::Grouped(groups) => {
                groups.into_iter().flat_map(|group| group.options).collect()
            }
            _ => Vec::new(),
        };
        return Offered {
            current: Some(select.current_value.to_string()),
            models: choices
                .into_iter()
                .take(MAX_MODELS)
                .map(|choice| ModelOption {
                    id: choice.value.to_string(),
                    name: choice.name,
                    description: choice.description,
                    ..ModelOption::default()
                })
                .collect(),
            switch: Some(ModelSwitch::ConfigOption(id)),
            ..Offered::default()
        };
    }
    let Some(models) = answer
        .get("models")
        .and_then(|models| serde_json::from_value::<SessionModels>(models.clone()).ok())
    else {
        return Offered::default();
    };
    Offered {
        current: models.current_model_id,
        models: models
            .available_models
            .into_iter()
            .take(MAX_MODELS)
            .map(|model| ModelOption {
                name: model.name.unwrap_or_else(|| model.model_id.clone()),
                id: model.model_id,
                description: model.description,
                ..ModelOption::default()
            })
            .collect(),
        switch: Some(ModelSwitch::SetModel),
        ..Offered::default()
    }
}

/// The request that switches `session` to `model` the way `switch` says.
pub(in crate::agents) fn switch_model(
    session: &SessionId,
    switch: &ModelSwitch,
    model: &str,
) -> Result<UntypedMessage, agent_client_protocol::Error> {
    match switch {
        ModelSwitch::ConfigOption(id) => UntypedMessage::new(
            AGENT_METHOD_NAMES.session_set_config_option,
            SetSessionConfigOptionRequest::new(session.clone(), id.clone(), model),
        ),
        ModelSwitch::SetModel => UntypedMessage::new(
            "session/set_model",
            serde_json::json!({ "sessionId": session, "modelId": model }),
        ),
    }
}

/// The name an ACP enum has on the wire, such as `end_turn`, through its
/// own serde conversion; empty for a value that is not a string there.
pub(in crate::agents) fn wire_name(value: &impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
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
    fn an_agent_without_models_offers_none() {
        let offered = offered_models(&serde_json::json!({"sessionId": "s1"}));
        assert_eq!(offered, Offered::default());
        // A broken `models` is ignored rather than failing the session.
        let broken = serde_json::json!({"sessionId": "s1", "models": {"availableModels": 3}});
        assert_eq!(offered_models(&broken), Offered::default());
        assert!(session_created(broken).is_ok());
    }
}
