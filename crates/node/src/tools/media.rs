//! The media and decision tools of `flashmind-tools` (images, video, speech,
//! 3D models, decisions), offered for each API key the user saved. The node
//! runs them with the keys it holds, so the keys never reach an agent.
//!
//! Agents see each flashmind tool the keys allow as it is, with
//! `list_models` for the models the configured providers offer.

use crate::process::spawn_env;
use flashmind_tools::ToolBuilder;
use flashmind_types::{ApiKey, ToolCall, ToolRegistry};
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
        let key =
            |key: ApiKey| spawn_env::secret(key.env_var()).map(|secret| secret.expose().to_owned());

        Self {
            tools: ToolBuilder::new().media(key, folder.clone()).build(),
            folder,
        }
    }

    /// The tools for `tools/list`.
    pub(super) fn listed(&self) -> Vec<Value> {
        self.names()
            .filter_map(|name| {
                let tool = self.tools.get(name)?;
                Some(json!({
                    "name": name,
                    "description": tool.description(),
                    "inputSchema": tool.parameters(),
                }))
            })
            .collect()
    }

    /// The line the server's instructions gain, when there are tools.
    pub(super) fn brief(&self) -> Option<String> {
        let names: Vec<&str> = self.names().filter(|name| *name != "list_models").collect();

        if names.is_empty() {
            return None;
        }

        Some(format!(
            "With the user's API keys this server also offers {}. They run on this machine and \
             save what they make in your working folder, at the path they give. `list_models` \
             lists the models you can pass as `model`.",
            names.join(", ")
        ))
    }

    /// Whether `name` is one of these tools.
    pub(super) fn offers(&self, name: &str) -> bool {
        self.names().any(|offered| offered == name)
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

    /// The tools the keys allow, sorted; `list_models` only beside another.
    fn names(&self) -> impl Iterator<Item = &str> + '_ {
        let names = self.tools.list();
        let any = names.iter().any(|name| *name != "list_models");

        names.into_iter().filter(move |_| any)
    }
}
