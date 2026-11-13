//! The media and decision tools of `flashmind-tools` (images, video, speech,
//! 3D models, decisions), offered for each API key the user saved. The node
//! runs them with the keys it holds, so the keys never reach an agent.
//!
//! Agents see one tool per kind of media, whose `action` picks the
//! flashmind tool, and whose `models` action lists the models the configured
//! providers offer for it.

use crate::machine::provision::spawn_env;
use flashmind_tools::{ToolBuilder, list_models::list_models};
use flashmind_types::{
    ApiKey, ModelCategory, Provider, ToolCall, ToolRegistry, llm::ProviderRegistry,
};
use serde_json::{Map, Value, json};
use std::{collections::BTreeSet, path::PathBuf, str::FromStr};
use tokio_util::sync::CancellationToken;

/// The action that lists a group's models.
const MODELS: &str = "models";
/// The arguments `models` takes.
const MODELS_ARGS: [&str; 2] = ["provider", "query"];
/// The most models one listing gives; a narrower query gives the rest.
const MAX_LISTED: usize = 200;

/// One tool as agents see it: an action per flashmind tool, and `models`.
struct Group {
    name: &'static str,
    what: &'static str,
    /// Each action and the flashmind tool it runs.
    actions: &'static [(&'static str, &'static str)],
    /// The model categories `models` lists.
    categories: &'static [ModelCategory],
}

const GROUPS: [Group; 4] = [
    Group {
        name: "image",
        what: "Images",
        actions: &[("generate", "generate_image"), ("edit", "image_edit")],
        categories: &[ModelCategory::ImageGeneration, ModelCategory::ImageEditing],
    },
    Group {
        name: "video",
        what: "Video",
        actions: &[("generate", "generate_video")],
        categories: &[ModelCategory::VideoGeneration],
    },
    Group {
        name: "speech",
        what: "Speech",
        actions: &[
            ("say", "tts"),
            ("transcribe", "transcribe"),
            ("voices", "list_voices"),
        ],
        categories: &[ModelCategory::Tts, ModelCategory::Stt],
    },
    Group {
        name: "model_3d",
        what: "3D models",
        actions: &[("generate", "generate_3d")],
        categories: &[ModelCategory::ThreeD],
    },
];

/// The flashmind tools agents reach other than through a group.
const ALONE: [&str; 1] = ["decision"];

/// The tools of one session, writing what they make into its folder.
pub(crate) struct Media {
    tools: ToolRegistry,
    providers: ProviderRegistry,
    folder: PathBuf,
}

impl Media {
    /// The tools the saved keys allow, for a session working in `folder`.
    pub(crate) fn new(folder: PathBuf) -> Self {
        let key = |key: ApiKey| {
            spawn_env::secret(key.env_var()).map(|secret| secret.expose().to_owned())
        };
        let builder = ToolBuilder::new().media(key, folder.clone());
        let providers = builder.providers().clone();
        Self {
            tools: builder.build(),
            providers,
            folder,
        }
    }

    /// The tools for `tools/list`.
    pub(super) fn listed(&self) -> Vec<Value> {
        let groups = self.groups().map(|group| self.group_tool(group));
        let alone = ALONE.into_iter().filter_map(|name| {
            let tool = self.tools.get(name)?;
            Some(json!({
                "name": name,
                "description": tool.description(),
                "inputSchema": tool.parameters(),
            }))
        });

        groups.chain(alone).collect()
    }

    /// The line the server's instructions gain, when there are tools.
    pub(super) fn brief(&self) -> Option<String> {
        let names: Vec<&str> = self
            .groups()
            .map(|group| group.name)
            .chain(ALONE.into_iter().filter(|name| self.tools.get(name).is_some()))
            .collect();

        if names.is_empty() {
            return None;
        }

        Some(format!(
            "\n\nWith the user's API keys it also offers {}. They run on this machine and \
             save what they make in your working folder, at the path they give. Each media \
             tool's `models` action lists the models you can pass as `model`.",
            names.join(", ")
        ))
    }

    /// Whether `name` is one of these tools.
    pub(super) fn offers(&self, name: &str) -> bool {
        self.groups().any(|group| group.name == name)
            || ALONE.contains(&name) && self.tools.get(name).is_some()
    }

    /// Runs `name` with `args`: the text for the agent, and whether it failed.
    pub(super) async fn call(&self, name: &str, args: Value) -> (String, bool) {
        let Some(group) = self.groups().find(|group| group.name == name) else {
            return self.run(name, args).await;
        };
        let Value::Object(mut args) = args else {
            return ("The arguments must be an object.".to_owned(), true);
        };
        let action = args.remove("action");
        let action = action.as_ref().and_then(Value::as_str).unwrap_or_default();

        if action == MODELS {
            if let Some(key) = args.keys().find(|key| !MODELS_ARGS.contains(&key.as_str())) {
                return (format!("`{key}` does not apply to {MODELS}."), true);
            }

            return self.models(group, &args).await;
        }

        match self.actions(group).find(|(named, _)| *named == action) {
            Some((_, tool)) => {
                // Another action's argument fails here; an unknown one fails, or goes to the
                // provider, in the tool itself.
                let own = self.properties(tool);
                let others: BTreeSet<String> = self
                    .actions(group)
                    .flat_map(|(_, other)| self.properties(other))
                    .chain(MODELS_ARGS.map(str::to_owned))
                    .collect();

                if let Some(key) = args.keys().find(|key| !own.contains(*key) && others.contains(*key)) {
                    return (format!("`{key}` does not apply to {action}."), true);
                }

                self.run(tool, Value::Object(args)).await
            }
            None => {
                let actions: Vec<&str> = self.actions(group).map(|(action, _)| action).collect();
                (
                    format!(
                        "Give `action`: one of {}, {MODELS}.",
                        actions.join(", ")
                    ),
                    true,
                )
            }
        }
    }

    /// The groups with at least one tool the keys allow.
    fn groups(&self) -> impl Iterator<Item = &'static Group> + '_ {
        GROUPS
            .iter()
            .filter(|group| self.actions(group).next().is_some())
    }

    /// The actions of `group` the keys allow, with their flashmind tools.
    fn actions<'g>(
        &self,
        group: &'g Group,
    ) -> impl Iterator<Item = (&'g str, &'g str)> + use<'g, '_> {
        group
            .actions
            .iter()
            .copied()
            .filter(|(_, tool)| self.tools.get(tool).is_some())
    }

    /// The arguments the flashmind tool `name` declares.
    fn properties(&self, name: &str) -> BTreeSet<String> {
        self.tools
            .get(name)
            .and_then(|tool| match tool.parameters().get("properties") {
                Some(Value::Object(own)) => Some(own.keys().cloned().collect()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// `group` as one MCP tool: its actions' descriptions and arguments
    /// together, with `action` to pick one.
    fn group_tool(&self, group: &Group) -> Value {
        let mut properties = Map::new();
        let mut names = Vec::new();
        let mut described = Vec::new();
        // Extra arguments are allowed only where a tool hands them to its provider.
        let mut open = false;

        for (action, tool) in self.actions(group) {
            let Some(tool) = self.tools.get(tool) else {
                continue;
            };
            names.push(action);
            open |= tool.parameters().get("additionalProperties") == Some(&Value::Bool(true));
            described.push(format!("{action}: {}", tool.description()));

            if let Some(Value::Object(own)) = tool.parameters().get("properties") {
                for (key, value) in own {
                    properties.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
        }

        let categories: Vec<String> = group.categories.iter().map(ToString::to_string).collect();
        names.push(MODELS);
        described.push(format!(
            "{MODELS}: lists the {} models the user's providers offer, live; optional provider \
             (such as openrouter) and query (part of a name or ID). Pass an ID it gives as `model`.",
            categories.join(" and ")
        ));
        properties.insert("action".into(), json!({ "type": "string", "enum": names }));
        properties
            .entry("provider".to_owned())
            .or_insert_with(|| json!({ "type": "string", "description": "models: only this provider" }));
        properties
            .entry("query".to_owned())
            .or_insert_with(|| json!({ "type": "string", "description": "models: part of a model's name or ID" }));

        json!({
            "name": group.name,
            "description": format!(
                "{} from the user's API keys, run on this machine. Actions:\n- {}",
                group.what,
                described.join("\n- ")
            ),
            "inputSchema": {
                "type": "object",
                "properties": properties,
                "required": ["action"],
                "additionalProperties": open,
            },
        })
    }

    /// The models of `group`'s categories, one ID per line.
    async fn models(&self, group: &Group, args: &Map<String, Value>) -> (String, bool) {
        let text = |key: &str| match args.get(key) {
            None => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.as_str())),
            Some(_) => Err(format!("`{key}` must be a string.")),
        };
        let (provider, query) = match (text("provider"), text("query")) {
            (Ok(provider), Ok(query)) => (provider, query),
            (Err(e), _) | (_, Err(e)) => return (e, true),
        };
        let provider = match provider.map(Provider::from_str) {
            Some(Ok(provider)) => Some(provider),
            Some(Err(_)) => return (format!("There is no provider {}.", provider.unwrap_or_default()), true),
            None => None,
        };
        let mut seen = BTreeSet::new();
        let mut lines = Vec::new();

        for &category in group.categories {
            let models =
                match list_models(&self.providers, Some(category), provider.as_ref(), query).await
                {
                    Ok(models) => models,
                    Err(e) => return (format!("Could not list the models: {e:#}"), true),
                };

            for model in models {
                if !seen.insert(model.id.clone()) {
                    continue;
                }

                lines.push(match &model.name {
                    Some(name) if *name != model.id => format!("{} ({name}, {category})", model.id),
                    _ => format!("{} ({category})", model.id),
                });
            }
        }

        if lines.is_empty() {
            return ("No models match.".to_owned(), false);
        }

        let more = lines.len().saturating_sub(MAX_LISTED);
        lines.truncate(MAX_LISTED);

        if more > 0 {
            lines.push(format!("and {more} more; give a query to narrow them."));
        }

        (lines.join("\n"), false)
    }

    /// Runs the flashmind tool `name`.
    async fn run(&self, name: &str, args: Value) -> (String, bool) {
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
