//! The media and decision tools of `flashmind-tools` (images, video, speech,
//! 3D models, decisions), offered for each API key the user saved. The node
//! runs them with the keys it holds, so the keys never reach an agent.

use crate::machine::provision::spawn_env;
use flashmind_tools::ToolBuilder;
use flashmind_types::{ToolCall, ToolRegistry};
use serde_json::{Value, json};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

/// The tools of one session, writing what they make into its folder.
pub(crate) struct Media {
    tools: ToolRegistry,
    folder: PathBuf,
}

impl Media {
    /// The tools the saved keys allow, for a session working in `folder`.
    pub(crate) fn new(folder: PathBuf) -> Self {
        let key = |key: flashmind_types::ApiKey| {
            spawn_env::secret(key.env_var()).map(|secret| secret.expose().to_owned())
        };
        let tools = ToolBuilder::new().media(key, folder.clone()).build();
        Self { tools, folder }
    }

    /// The tools for `tools/list`.
    pub(super) fn listed(&self) -> Vec<Value> {
        self.tools
            .definitions()
            .into_iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "inputSchema": tool.parameters,
                })
            })
            .collect()
    }

    /// The line the server's instructions gain, when there are tools.
    pub(super) fn brief(&self) -> Option<String> {
        let names = self.tools.list();

        if names.is_empty() {
            return None;
        }

        Some(format!(
            "\n\nWith the user's API keys: {}. They run on this machine and save what they \
             make in your working folder, at the path they give.",
            names.join(", ")
        ))
    }

    /// Whether `name` is one of these tools.
    pub(super) fn offers(&self, name: &str) -> bool {
        self.tools.get(name).is_some()
    }

    /// Runs `name` with `args`: the text for the agent, and whether it failed.
    pub(super) async fn call(&self, name: &str, args: Value) -> (String, bool) {
        let call = ToolCall {
            id: name.to_owned(),
            name: name.to_owned(),
            arguments: args,
        };
        let result = self
            .tools
            .execute(&call, Some(&self.folder), &CancellationToken::new())
            .await;

        (result.output(), !result.is_success())
    }
}
