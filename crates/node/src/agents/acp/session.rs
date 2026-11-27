//! Session startup, capabilities and model configuration.

use crate::logs::subagent::CLAUDE_SPAWNS;
use agent_client_protocol::{
    UntypedMessage,
    schema::ProtocolVersion,
    v1::schema::{
        AGENT_METHOD_NAMES, ClientCapabilities, ClientSessionCapabilities, CompactionCapabilities,
        ContentBlock, ElicitationCapabilities, ElicitationFormCapabilities, Implementation,
        InitializeRequest, InitializeResponse, LoadSessionRequest, McpServer, Meta,
        NewSessionRequest, NewSessionResponse, ResumeSessionRequest, SessionConfigId,
        SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
        SessionConfigSelectOption, SessionConfigSelectOptions, SessionId, SessionModeState,
        SetSessionConfigOptionRequest, SystemPrompt, TextContent,
    },
};
use beep_proto::node::agent::{Effort, ModeOption, ModelOption};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::path::Path;

/// What `initialize` sends: this client reads and writes no files and runs
/// no terminals for the agent (the crate's defaults), so it does that work
/// itself. It takes `compaction_update`s, which the transcript shows, and
/// form elicitations, which the user answers as questions. With
/// `async_tasks` it also says, in the AIR extension Claude Code's adapter
/// reads, that it takes the commands the agent runs in the background
/// (`async_task_*` updates, see `AsyncTask`) and can stop them.
pub(in crate::agents) fn initialize(async_tasks: bool) -> InitializeRequest {
    let session = ClientSessionCapabilities::new().compaction(CompactionCapabilities::new());
    // Form elicitation lets Claude Code ask its own questions
    // (AskUserQuestion), which its adapter turns off for a client without it.
    let elicitation = ElicitationCapabilities::new().form(ElicitationFormCapabilities::new());
    let mut capabilities = ClientCapabilities::new()
        .session(session)
        .elicitation(elicitation);

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
pub(in crate::agents) const MAX_MODELS: usize = 4000;

/// The most session modes kept from one agent's list.
const MAX_MODES: usize = 32;

/// The id of the mode where an agent plans and changes nothing.
pub(in crate::agents) const PLAN_MODE: &str = "plan";

/// The models an agent offers in its answer to `session/new`, and how it
/// switches between them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(in crate::agents) struct Offered {
    pub(in crate::agents) fast: Option<SessionConfigId>,
    /// The model the session runs on, when it says.
    pub(in crate::agents) current: Option<String>,
    pub(in crate::agents) models: Vec<ModelOption>,
    pub(in crate::agents) switch: Option<ModelSwitch>,
    /// Its config option of category `thought_level`, which sets how hard
    /// it thinks, when it has one.
    pub(in crate::agents) thinking: Option<SessionConfigId>,
    /// The session modes it offers and the one it is in, when it has any.
    pub(in crate::agents) modes: Option<Modes>,
    pub(in crate::agents) mode_config: Option<SessionConfigId>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::agents) enum SystemPromptFormat {
    Acp,
    Antigravity,
}

impl SystemPromptFormat {
    pub(in crate::agents) fn append(self, text: &str) -> AppendedSystemPrompt<'_> {
        AppendedSystemPrompt { format: self, text }
    }
}

pub(in crate::agents) struct AppendedSystemPrompt<'a> {
    format: SystemPromptFormat,
    text: &'a str,
}

/// How the agent advertises append support in its v1 `initialize` answer.
pub(super) fn system_prompt_format(initialized: &InitializeResponse) -> Option<SystemPromptFormat> {
    let capabilities = &initialized.agent_capabilities;
    if capabilities.session_capabilities.system_prompt.is_some() {
        return Some(SystemPromptFormat::Acp);
    }

    capabilities
        .meta
        .as_ref()?
        .get("antigravity")?
        .pointer("/systemPrompt/append")?
        .as_bool()?
        .then_some(SystemPromptFormat::Antigravity)
}

/// What every session asks of its agent in `_meta`. Claude Code's adapter
/// passes `claudeCode.options` on to Claude Code: `perTaskStopAffordance`
/// says the client stops background tasks one by one (`stop_task`), so
/// `session/cancel` ends the main turn and spares background subagents and
/// workflows, which otherwise end with it. Without `subagents`, as for the
/// home assistant, whose threads stand in for them, Claude Code drops its
/// tools that start one and flsh its own (`flsh.subagents`). Other agents
/// ignore what is not theirs.
fn session_options(
    subagents: bool,
    brief: Option<AppendedSystemPrompt<'_>>,
) -> (Meta, Option<SystemPrompt>) {
    let mut options = json!({"perTaskStopAffordance": true});
    let mut meta = Meta::new();

    if !subagents {
        options["disallowedTools"] = json!(CLAUDE_SPAWNS);
        meta.insert("flsh".to_owned(), json!({"subagents": false}));
    }

    meta.insert("claudeCode".to_owned(), json!({"options": options}));

    let system = brief.and_then(|brief| match brief.format {
        SystemPromptFormat::Acp => Some(SystemPrompt::new(vec![ContentBlock::Text(
            TextContent::new(brief.text),
        )])),
        SystemPromptFormat::Antigravity => {
            meta.insert(
                "antigravity".to_owned(),
                json!({"systemPrompt": {"append": brief.text}}),
            );
            None
        }
    });
    (meta, system)
}

/// `session/new` in `cwd`, with `brief` appended to the agent's system
/// prompt, sent untyped so the answer keeps `models`, which the crate's
/// typed answer drops.
pub(in crate::agents) fn new_session(
    cwd: &Path,
    mcp_servers: Vec<McpServer>,
    brief: Option<AppendedSystemPrompt<'_>>,
    subagents: bool,
) -> Result<UntypedMessage, agent_client_protocol::Error> {
    let (meta, system) = session_options(subagents, brief);
    UntypedMessage::new(
        AGENT_METHOD_NAMES.session_new,
        NewSessionRequest::new(cwd)
            .mcp_servers(mcp_servers)
            .system_prompt(system)
            .meta(meta),
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
    brief: Option<AppendedSystemPrompt<'_>>,
    subagents: bool,
) -> Result<UntypedMessage, agent_client_protocol::Error> {
    let (meta, system) = session_options(subagents, brief);

    match method {
        ResumeMethod::Load => UntypedMessage::new(
            AGENT_METHOD_NAMES.session_load,
            LoadSessionRequest::new(session, cwd)
                .mcp_servers(mcp_servers)
                .system_prompt(system)
                .meta(meta),
        ),
        ResumeMethod::Resume => UntypedMessage::new(
            AGENT_METHOD_NAMES.session_resume,
            ResumeSessionRequest::new(session, cwd)
                .mcp_servers(mcp_servers)
                .system_prompt(system)
                .meta(meta),
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

/// The text pi's adapter sends as the session's first message (its
/// version, context files and skills), which the session leaves out.
pub(in crate::agents) fn startup_banner(answer: &Value) -> Option<String> {
    answer
        .pointer("/_meta/piAcp/startupInfo")?
        .as_str()
        .map(str::to_owned)
}

/// The models and thinking option an answer to `session/new`,
/// `session/load` or `session/resume` offers.
pub(in crate::agents) fn offers(answer: &Value) -> Offered {
    let options = config_options(answer);
    Offered {
        fast: options
            .iter()
            .find(|o| o.id.to_string() == "fast-mode")
            .map(|o| o.id.clone()),
        thinking: thinking_option(&options),
        modes: session_modes(answer).filter(|modes| !thinking_levels(modes)),
        ..models_from_options(answer, options)
    }
}

/// pi's adapter offers its thinking levels as modes too, which the
/// thinking option already switches.
fn thinking_levels(modes: &Modes) -> bool {
    modes
        .available
        .iter()
        .all(|mode| mode.id.parse::<Effort>().is_ok())
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

pub(in crate::agents) fn set_fast(
    session: &SessionId,
    id: &SessionConfigId,
    fast: bool,
) -> Result<UntypedMessage, agent_client_protocol::Error> {
    UntypedMessage::new(
        AGENT_METHOD_NAMES.session_set_config_option,
        SetSessionConfigOptionRequest::new(
            session.clone(),
            id.clone(),
            if fast { "on" } else { "off" },
        ),
    )
}

/// The models an answer offers: its config option of
/// category `model` when it has one, else its unstable `models`.
#[cfg(test)]
pub(in crate::agents) fn offered_models(answer: &Value) -> Offered {
    models_from_options(answer, config_options(answer))
}

fn models_from_options(answer: &Value, options: Vec<SessionConfigOption>) -> Offered {
    let has_thinking = thinking_option(&options).is_some();
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
        let mut models: Vec<ModelOption> = choices
            .into_iter()
            .take(MAX_MODELS)
            .map(|choice| ModelOption {
                id: choice.value.to_string(),
                name: choice.name,
                description: choice.description,
                ..ModelOption::default()
            })
            .collect();

        if has_thinking {
            attach_model_efforts(answer, &mut models);
        }

        return Offered {
            current: Some(select.current_value.to_string()),
            models,
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

/// Canonical order for thinking levels on a model option.
const EFFORT_ORDER: &[Effort] = &[
    Effort::Off,
    Effort::None,
    Effort::Minimal,
    Effort::Low,
    Effort::Medium,
    Effort::High,
    Effort::Xhigh,
    Effort::Max,
    Effort::Ultra,
];

/// Reads per-model thinking levels when an agent's unstable `models` list
/// pairs each base model from `configOptions` with `<model>-<effort>` entries
/// (as Antigravity does).
fn attach_model_efforts(answer: &Value, models: &mut [ModelOption]) {
    let Some(session_models) = answer
        .get("models")
        .and_then(|models| serde_json::from_value::<SessionModels>(models.clone()).ok())
    else {
        return;
    };
    let known: std::collections::HashSet<&str> = models.iter().map(|m| m.id.as_str()).collect();
    let variants: Vec<&str> = session_models
        .available_models
        .iter()
        .map(|m| m.model_id.as_str())
        .filter(|id| !known.contains(id))
        .collect();

    if variants.is_empty() {
        return;
    }

    for model in models {
        let prefix = format!("{}-", model.id);
        let found: Vec<Effort> = variants
            .iter()
            .filter_map(|variant| variant.strip_prefix(&prefix)?.parse::<Effort>().ok())
            .collect();

        if found.is_empty() {
            continue;
        }

        model.efforts = EFFORT_ORDER
            .iter()
            .copied()
            .filter(|effort| found.contains(effort))
            .collect();
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
#[path = "../../tests/agents_acp_session.rs"]
mod tests;
