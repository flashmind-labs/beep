use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::str::FromStr;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ident::{Permission, PluginId};
use crate::state::PluginState;

/// The most tools one plugin declares.
pub const MAX_TOOLS: usize = 8;
/// The longest tool description, in characters.
pub const MAX_DESCRIPTION: usize = 300;
/// The most JSON text a tool takes or returns, in bytes.
pub(crate) const MAX_JSON_BYTES: usize = 64 * 1024;
/// How long a tool's promise may stay unsettled.
#[cfg(not(test))]
pub(crate) const TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
pub(crate) const TIMEOUT: Duration = Duration::from_millis(500);
/// Calls that wait for one plugin's thread.
pub(crate) const MAX_PENDING: usize = 16;

const NAME_LEN: std::ops::RangeInclusive<usize> = 1..=41;

#[derive(Debug, Error)]
#[error(
    "invalid tool name {0:?}: use lowercase letters, digits and underscores, starting with a letter, at most 41 characters"
)]
pub struct InvalidToolName(String);

/// The name of a tool a plugin provides.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ToolName(String);

impl FromStr for ToolName {
    type Err = InvalidToolName;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut chars = text.chars();
        let starts_with_letter = chars.next().is_some_and(|c| c.is_ascii_lowercase());
        let rest_ok = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');

        if NAME_LEN.contains(&text.len()) && starts_with_letter && rest_ok {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidToolName(text.to_owned()))
        }
    }
}

impl TryFrom<String> for ToolName {
    type Error = InvalidToolName;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<ToolName> for String {
    fn from(name: ToolName) -> Self {
        name.0
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A tool a manifest declares: coding agents call it by name with arguments
/// that fit `input`, a JSON Schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSpec {
    pub name: ToolName,
    #[serde(default)]
    pub description: String,
    #[serde(default = "any_object")]
    pub input: serde_json::Value,
}

fn any_object() -> serde_json::Value {
    serde_json::json!({ "type": "object" })
}

#[derive(Debug, Error)]
pub enum ToolsError {
    #[error("tools need the tools permission")]
    Permission,
    #[error("a plugin has at most {MAX_TOOLS} tools, this one declares {0}")]
    TooMany(usize),
    #[error("tool {0} is declared twice")]
    Twice(ToolName),
    #[error("the description of tool {0} is over {MAX_DESCRIPTION} characters")]
    LongDescription(ToolName),
    #[error("the input schema of tool {name} is not valid: {reason}")]
    Schema { name: ToolName, reason: String },
}

/// Checks the tools a manifest declares.
pub(crate) fn check_all(tools: &[ToolSpec], permissions: &[Permission]) -> Result<(), ToolsError> {
    if tools.is_empty() {
        return Ok(());
    }
    if !permissions.contains(&Permission::Tools) {
        return Err(ToolsError::Permission);
    }
    if tools.len() > MAX_TOOLS {
        return Err(ToolsError::TooMany(tools.len()));
    }

    let mut seen = BTreeSet::new();
    for tool in tools {
        if !seen.insert(&tool.name) {
            return Err(ToolsError::Twice(tool.name.clone()));
        }
        if tool.description.chars().count() > MAX_DESCRIPTION {
            return Err(ToolsError::LongDescription(tool.name.clone()));
        }
        jsonschema::validator_for(&tool.input).map_err(|e| ToolsError::Schema {
            name: tool.name.clone(),
            reason: e.to_string(),
        })?;
    }

    Ok(())
}

impl ToolSpec {
    /// Whether `input` fits the tool's schema.
    pub(crate) fn check_input(&self, input: &serde_json::Value) -> Result<(), ToolError> {
        let validator =
            jsonschema::validator_for(&self.input).map_err(|e| ToolError::Input(e.to_string()))?;

        validator
            .validate(input)
            .map_err(|e| ToolError::Input(format!("{e} (at {})", e.instance_path())))
    }
}

/// Why a tool call gave no result.
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("there is no plugin {0}")]
    NoPlugin(PluginId),
    #[error("plugin {plugin} is {state}, so it answers no tool calls")]
    NotRunning {
        plugin: PluginId,
        state: PluginState,
    },
    #[error("plugin {plugin} has no tool {tool}; it provides: {}", list(.provided))]
    NoTool {
        plugin: PluginId,
        tool: ToolName,
        provided: Vec<ToolName>,
    },
    #[error("the arguments do not fit the tool: {0}")]
    Input(String),
    #[error("the tool failed: {0}")]
    Failed(String),
    #[error("the tool gave no answer within {} s", TIMEOUT.as_secs())]
    TimedOut,
    #[error("the tool's result is over {MAX_JSON_BYTES} bytes")]
    TooLarge,
    #[error("the plugin has too many calls waiting")]
    Busy,
    #[error("the plugin stopped before it answered")]
    Stopped,
}

fn list(names: &[ToolName]) -> String {
    if names.is_empty() {
        return "none".to_owned();
    }

    names
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// One call for a plugin's thread, answered through `reply` as JSON text.
pub(crate) struct ToolCall {
    pub(crate) name: ToolName,
    pub(crate) input: String,
    pub(crate) reply: Sender<Result<String, ToolError>>,
}

/// The tools of one sandbox: the ones its manifest declares, the ones its
/// code registered, and the calls waiting for a promise.
pub(crate) struct Tools {
    declared: BTreeSet<ToolName>,
    registered: BTreeSet<ToolName>,
    pending: HashMap<u32, (Sender<Result<String, ToolError>>, Instant)>,
    next: u32,
}

impl Tools {
    pub(crate) fn new(declared: &[ToolSpec]) -> Self {
        Self {
            declared: declared.iter().map(|tool| tool.name.clone()).collect(),
            registered: BTreeSet::new(),
            pending: HashMap::new(),
            next: 0,
        }
    }

    /// Records `beep.tool(name, ...)`: an empty string, or why it is refused.
    pub(crate) fn register(&mut self, name: &str) -> String {
        let name = match name.parse::<ToolName>() {
            Ok(name) => name,
            Err(error) => return error.to_string(),
        };
        if !self.declared.contains(&name) {
            return format!("tool {name} is not declared in the manifest");
        }
        if !self.registered.insert(name.clone()) {
            return format!("tool {name} is registered twice");
        }

        String::new()
    }

    /// The declared tools that no code registered, as a list.
    pub(crate) fn unregistered(&self) -> Option<String> {
        let missing: Vec<ToolName> = self
            .declared
            .difference(&self.registered)
            .cloned()
            .collect();

        (!missing.is_empty()).then(|| list(&missing))
    }

    /// Waits for the answer of a call, which `settle` gives or `expire` ends.
    pub(crate) fn start(&mut self, call: ToolCall, now: Instant) -> u32 {
        self.next += 1;
        self.pending.insert(self.next, (call.reply, now + TIMEOUT));

        self.next
    }

    /// The answer of call `id`: its JSON result, or why it failed.
    pub(crate) fn settle(&mut self, id: u32, ok: bool, text: String) {
        let Some((reply, _)) = self.pending.remove(&id) else {
            return;
        };
        let result = match (ok, text.len() > MAX_JSON_BYTES) {
            (false, _) => Err(ToolError::Failed(text)),
            (true, true) => Err(ToolError::TooLarge),
            (true, false) => Ok(text),
        };

        let _ = reply.send(result);
    }

    /// Ends the calls whose time ran out at `now`.
    pub(crate) fn expire(&mut self, now: Instant) {
        self.pending.retain(|_, (reply, due)| {
            let over = *due <= now;
            if over {
                let _ = reply.send(Err(ToolError::TimedOut));
            }

            !over
        });
    }

    pub(crate) fn next_due(&self) -> Option<Instant> {
        self.pending.values().map(|(_, due)| *due).min()
    }
}
