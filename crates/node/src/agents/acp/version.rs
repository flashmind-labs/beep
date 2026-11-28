//! Version negotiation and the session operations shared by both ACP versions.

use super::{
    Offered, ResumeMethod,
    prompt::{takes_images, takes_steering},
    session::{self, resume_method},
};
use agent_client_protocol::{
    Error, JsonRpcMessage, UntypedMessage, v1::schema as v1, v2::schema as v2,
};
use serde_json::{Value, json};
use session::SystemPromptFormat;
use std::collections::BTreeMap;

/// V1 by default. A harness opts into v2, with negotiated v1 fallback,
/// through its launch environment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::agents) enum Protocol {
    #[default]
    V1,
    V2,
}

pub(in crate::agents) struct Capabilities {
    pub protocol: Protocol,
    pub images: bool,
    pub steering: bool,
    pub system_prompt: Option<SystemPromptFormat>,
    pub resume: Option<ResumeMethod>,
}

impl Protocol {
    pub fn from_env(env: &BTreeMap<String, String>) -> anyhow::Result<Self> {
        match env.get("BEEP_ACP_VERSION").map(String::as_str) {
            None | Some("1") => Ok(Self::V1),
            Some("2") => Ok(Self::V2),
            Some(_) => anyhow::bail!("BEEP_ACP_VERSION must be 1 or 2"),
        }
    }

    /// Only these shared operations have different request shapes. Prompt
    /// acceptance and session updates are handled separately by each protocol.
    pub fn request(self, request: UntypedMessage) -> Result<UntypedMessage, Error> {
        if self == Self::V1 {
            return Ok(request);
        }

        let mut params = request.params().clone();
        let method = match request.method() {
            "initialize" => {
                let previous = v1::InitializeRequest::parse_message("initialize", &params)?;
                let mut capabilities = json!({});
                if let Some(meta) = previous.client_capabilities.meta {
                    capabilities["_meta"] = json!(meta);
                }
                // V1 peers read the legacy advertisement when they select
                // v1. V2 peers ignore those unknown fields and use the v2
                // advertisement, so fallback keeps the client's extensions.
                params["protocolVersion"] = json!(2);
                params["info"] = json!(previous.client_info);
                params["capabilities"] = capabilities;
                // Validate our advertisement against the SDK's v2 schema.
                v2::InitializeRequest::parse_message("initialize", &params)?;
                "initialize"
            }
            "session/load" => {
                tag_stdio_servers(&mut params);
                params["replayFrom"] = json!({"type": "start"});
                v2::ResumeSessionRequest::parse_message("session/resume", &params)?;
                "session/resume"
            }
            "session/set_config_option" => {
                let id = params
                    .as_object_mut()
                    .and_then(|p| p.remove("configId").or_else(|| p.remove("id")))
                    .ok_or_else(Error::invalid_params)?;
                params["configId"] = id;
                params["type"] = json!("id");
                v2::SetSessionConfigOptionRequest::parse_message(request.method(), &params)?;
                request.method()
            }
            "session/new" => {
                tag_stdio_servers(&mut params);
                v2::NewSessionRequest::parse_message(request.method(), &params)?;
                request.method()
            }
            _ => request.method(),
        };

        UntypedMessage::new(method, params)
    }

    pub fn initialized(self, answer: Value) -> Result<Capabilities, Error> {
        let version = answer.get("protocolVersion").and_then(Value::as_u64);
        match (self, version) {
            (_, Some(1)) => {
                let initialized: v1::InitializeResponse =
                    serde_json::from_value(answer).map_err(invalid)?;
                Ok(Capabilities {
                    protocol: Self::V1,
                    images: takes_images(&initialized),
                    steering: takes_steering(&initialized),
                    system_prompt: session::system_prompt_format(&initialized),
                    resume: resume_method(&initialized),
                })
            }
            (Self::V2, Some(2)) => {
                let initialized: v2::InitializeResponse =
                    serde_json::from_value(answer).map_err(invalid)?;
                let session = initialized.capabilities.session.as_ref();
                if session.is_none() {
                    return Err(
                        Error::invalid_params().data("the agent does not support ACP sessions")
                    );
                }
                Ok(Capabilities {
                    protocol: Self::V2,
                    images: session
                        .and_then(|s| s.prompt.as_ref())
                        .is_some_and(|p| p.image.is_some()),
                    steering: false,
                    system_prompt: session
                        .and_then(|s| s.system_prompt.as_ref())
                        .map(|_| SystemPromptFormat::Acp),
                    resume: Some(ResumeMethod::Load),
                })
            }
            _ => Err(Error::invalid_params().data("the agent selected an unsupported ACP version")),
        }
    }

    pub fn offers(self, answer: &Value) -> Offered {
        if self == Self::V1 {
            return session::offers(answer);
        }

        let mut offered = Offered::default();
        for option in options(answer) {
            let v2::SessionConfigKind::Select(select) = option.kind else {
                continue;
            };
            let choices = match select.options {
                v2::SessionConfigSelectOptions::Ungrouped(choices) => choices,
                v2::SessionConfigSelectOptions::Grouped(groups) => {
                    groups.into_iter().flat_map(|g| g.options).collect()
                }
                _ => continue,
            };
            let id = v1::SessionConfigId::new(option.config_id.to_string());
            match option.category {
                Some(v2::SessionConfigOptionCategory::Model) => {
                    offered.current = Some(select.current_value.to_string());
                    offered.switch = Some(session::ModelSwitch::ConfigOption(id));
                    offered.models = choices
                        .into_iter()
                        .take(super::MAX_MODELS)
                        .map(|choice| beep_proto::node::agent::ModelOption {
                            id: choice.value.to_string(),
                            name: choice.name,
                            description: choice.description,
                            ..Default::default()
                        })
                        .collect();
                }
                Some(v2::SessionConfigOptionCategory::ThoughtLevel) => offered.thinking = Some(id),
                _ if id.to_string() == "fast-mode" => offered.fast = Some(id),
                Some(v2::SessionConfigOptionCategory::Mode) => {
                    offered.mode_config = Some(id);
                    offered.modes = Some(session::Modes {
                        current: select.current_value.to_string(),
                        available: choices
                            .into_iter()
                            .take(32)
                            .map(|choice| beep_proto::node::agent::ModeOption {
                                id: choice.value.to_string(),
                                name: choice.name,
                            })
                            .collect(),
                    });
                }
                _ => {}
            }
        }
        offered
    }
}

/// V1 stdio MCP servers carry no `type`; v2 requires `"type": "stdio"`.
fn tag_stdio_servers(params: &mut Value) {
    let servers = params.get_mut("mcpServers").and_then(Value::as_array_mut);
    for server in servers.into_iter().flatten() {
        if let Some(server) = server.as_object_mut() {
            server.entry("type").or_insert_with(|| json!("stdio"));
        }
    }
}

pub(super) fn options(answer: &Value) -> Vec<v2::SessionConfigOption> {
    answer
        .get("configOptions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| serde_json::from_value(option.clone()).ok())
        .collect()
}

pub(in crate::agents) fn invalid(error: serde_json::Error) -> Error {
    Error::invalid_params().data(error.to_string())
}

#[cfg(test)]
#[path = "../../tests/agents_acp_version.rs"]
mod tests;
