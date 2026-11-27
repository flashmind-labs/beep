//! A session's settings, offered as Codex's adapter offers them: the mode (an
//! approval and sandbox preset), the collaboration mode, the model, its
//! reasoning effort and fast mode; and what they ask of a thread and a turn.

use serde::Deserialize;
use serde_json::{Map, Value, json};
use strum::{AsRefStr, EnumString};

/// The config options, by id.
const MODE: &str = "mode";
const COLLABORATION: &str = "collaboration_mode";
const MODEL: &str = "model";
const EFFORT: &str = "reasoning_effort";
const FAST: &str = "fast-mode";
/// The mode a session starts in when the node names one.
pub(super) const INITIAL_MODE: &str = "INITIAL_AGENT_MODE";

/// An approval and sandbox preset, as Codex's adapter names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, AsRefStr)]
pub(super) enum Mode {
    #[strum(serialize = "read-only")]
    ReadOnly,
    #[strum(serialize = "workspace-write")]
    WorkspaceWrite,
    #[strum(serialize = "agent")]
    Agent,
    #[strum(serialize = "agent-full-access")]
    FullAccess,
}

const MODES: [Mode; 4] = [
    Mode::ReadOnly,
    Mode::WorkspaceWrite,
    Mode::Agent,
    Mode::FullAccess,
];

impl Mode {
    /// The mode `asked` names, as the node sets `INITIAL_MODE`, else
    /// Codex's adapter's default.
    pub(super) fn initial(asked: Option<&str>) -> Self {
        asked
            .and_then(|mode| mode.parse().ok())
            .unwrap_or(Self::Agent)
    }

    fn name(self) -> &'static str {
        match self {
            Self::ReadOnly => "Read-only",
            Self::WorkspaceWrite => "Workspace access",
            Self::Agent => "Auto review",
            Self::FullAccess => "Full access",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::ReadOnly => "Requires approval to edit files and access the internet.",
            Self::WorkspaceWrite => {
                "Edit workspace files; ask before writing outside the workspace or accessing the network."
            }
            Self::Agent => "Only ask for actions detected as potentially unsafe",
            Self::FullAccess => "Unrestricted access to the internet and any file on your computer",
        }
    }

    fn approval_policy(self) -> &'static str {
        match self {
            Self::FullAccess => "never",
            _ => "on-request",
        }
    }

    fn approvals_reviewer(self) -> &'static str {
        match self {
            Self::Agent => "auto_review",
            _ => "user",
        }
    }

    fn sandbox(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite | Self::Agent => "workspace-write",
            Self::FullAccess => "danger-full-access",
        }
    }

    /// Its approval and sandbox fields of `thread/start` and `thread/resume`.
    pub(super) fn thread(self) -> Map<String, Value> {
        let mut fields = Map::new();
        fields.insert("approvalPolicy".to_owned(), json!(self.approval_policy()));
        fields.insert(
            "approvalsReviewer".to_owned(),
            json!(self.approvals_reviewer()),
        );
        fields.insert("sandbox".to_owned(), json!(self.sandbox()));
        fields
    }

    fn sandbox_policy(self) -> Value {
        match self {
            Self::ReadOnly => json!({"type": "readOnly", "networkAccess": false}),
            Self::WorkspaceWrite | Self::Agent => json!({
                "type": "workspaceWrite",
                "writableRoots": [],
                "networkAccess": false,
                "excludeTmpdirEnvVar": false,
                "excludeSlashTmp": false,
            }),
            Self::FullAccess => json!({"type": "dangerFullAccess"}),
        }
    }
}

/// How Codex works with the user: plan first, or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, AsRefStr)]
pub(super) enum Collaboration {
    #[strum(serialize = "default")]
    Default,
    #[strum(serialize = "plan")]
    Plan,
}

/// A model `model/list` offers.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Model {
    id: String,
    display_name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    is_default: bool,
    default_reasoning_effort: String,
    #[serde(default)]
    supported_reasoning_efforts: Vec<EffortOption>,
    #[serde(default)]
    additional_speed_tiers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EffortOption {
    reasoning_effort: String,
    #[serde(default)]
    description: String,
}

impl Model {
    fn efforts(&self) -> impl Iterator<Item = &str> {
        self.supported_reasoning_efforts
            .iter()
            .map(|option| option.reasoning_effort.as_str())
    }

    fn fast(&self) -> bool {
        self.additional_speed_tiers
            .iter()
            .any(|tier| tier == "fast")
    }
}

/// What a thread starts or resumes with, as Codex answers it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Started {
    #[serde(default)]
    pub(super) model: String,
    #[serde(default)]
    pub(super) reasoning_effort: Option<String>,
    #[serde(default)]
    pub(super) service_tier: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Settings {
    pub(super) mode: Mode,
    pub(super) collaboration: Collaboration,
    models: Vec<Model>,
    model: String,
    effort: Option<String>,
    fast: bool,
}

impl Settings {
    pub(super) fn new(mode: Mode, models: Vec<Model>, started: Started) -> Self {
        let model = Some(started.model)
            .filter(|model| !model.is_empty())
            .or_else(|| {
                models
                    .iter()
                    .find(|model| model.is_default)
                    .map(|model| model.id.clone())
            })
            .or_else(|| models.first().map(|model| model.id.clone()))
            .unwrap_or_default();
        let mut settings = Self {
            mode,
            collaboration: Collaboration::Default,
            models,
            model,
            effort: started.reasoning_effort,
            fast: matches!(started.service_tier.as_deref(), Some("fast" | "priority")),
        };
        settings.fit_effort();
        settings
    }

    fn current(&self) -> Option<&Model> {
        self.models.iter().find(|model| model.id == self.model)
    }

    /// Keeps the effort when the model takes it, else takes the model's
    /// default.
    fn fit_effort(&mut self) {
        let Some(model) = self.current() else {
            return;
        };
        let fits = self
            .effort
            .as_deref()
            .is_some_and(|effort| model.efforts().any(|offered| offered == effort));

        if !fits {
            self.effort = Some(model.default_reasoning_effort.clone());
        }
    }

    /// The model the session runs on.
    pub(super) fn model(&self) -> &str {
        &self.model
    }

    /// Sets the mode `id`.
    pub(super) fn set_mode(&mut self, id: &str) -> Result<(), String> {
        self.mode = id.parse().map_err(|_| format!("no mode {id}"))?;
        Ok(())
    }

    /// Sets the config option `id` to `value`.
    pub(super) fn set(&mut self, id: &str, value: &str) -> Result<(), String> {
        match id {
            MODE => self.set_mode(value)?,
            COLLABORATION => {
                self.collaboration = value
                    .parse()
                    .map_err(|_| format!("no collaboration mode {value}"))?;
            }
            MODEL => {
                if !self.models.iter().any(|model| model.id == value) {
                    return Err(format!("no model {value}"));
                }
                self.model = value.to_owned();
                self.fit_effort();
            }
            EFFORT => {
                let offered = self
                    .current()
                    .is_some_and(|model| model.efforts().any(|effort| effort == value));
                if !offered {
                    return Err(format!("{} takes no effort {value}", self.model));
                }
                self.effort = Some(value.to_owned());
            }
            FAST => match value {
                "on" => self.fast = true,
                "off" => self.fast = false,
                _ => return Err(format!("fast mode is on or off, not {value}")),
            },
            _ => return Err(format!("no option {id}")),
        }

        Ok(())
    }

    /// The modes, as `modes` in an answer to `session/new`.
    pub(super) fn modes(&self) -> Value {
        let available: Vec<Value> = MODES
            .iter()
            .map(|mode| json!({"id": mode.as_ref(), "name": mode.name(), "description": mode.description()}))
            .collect();
        json!({"availableModes": available, "currentModeId": self.mode.as_ref()})
    }

    /// The config options, as `configOptions` in an answer.
    pub(super) fn options(&self) -> Value {
        let modes: Vec<Value> = MODES
            .iter()
            .map(|mode| json!({"value": mode.as_ref(), "name": mode.name(), "description": mode.description()}))
            .collect();
        let mut options = vec![
            select(
                MODE,
                "Mode",
                "Approval and sandboxing preset for the session",
                "mode",
                self.mode.as_ref(),
                modes,
            ),
            select(
                COLLABORATION,
                "Collaboration mode",
                "How Codex collaborates for subsequent turns",
                COLLABORATION,
                self.collaboration.as_ref(),
                vec![
                    json!({"value": "default", "name": "Default"}),
                    json!({"value": "plan", "name": "Plan", "description": "Plan before making changes"}),
                ],
            ),
        ];

        if self.models.is_empty() {
            return Value::Array(options);
        }

        let models = self
            .models
            .iter()
            .map(|model| json!({"value": model.id, "name": display_name(&model.display_name), "description": model.description}))
            .collect();
        options.push(select(
            MODEL,
            "Model",
            "Model Codex uses for the session",
            "model",
            &self.model,
            models,
        ));

        if let (Some(model), Some(effort)) = (self.current(), &self.effort) {
            let efforts = model
                .supported_reasoning_efforts
                .iter()
                .map(|option| {
                    json!({
                        "value": option.reasoning_effort,
                        "name": capitalized(&option.reasoning_effort),
                        "description": option.description,
                    })
                })
                .collect();
            options.push(select(
                EFFORT,
                "Reasoning effort",
                "How much reasoning effort the model should use",
                "thought_level",
                effort,
                efforts,
            ));
        }

        if self.current().is_some_and(Model::fast) {
            let description = "1.5x speed, increased usage";
            options.push(select(
                FAST,
                "Fast mode",
                description,
                "model_config",
                if self.fast { "on" } else { "off" },
                vec![
                    json!({"value": "off", "name": "Off", "description": "Default speed, normal usage"}),
                    json!({"value": "on", "name": "On", "description": description}),
                ],
            ));
        }

        Value::Array(options)
    }

    /// The settings fields of `turn/start`, which hold for later turns too.
    pub(super) fn turn(&self) -> Map<String, Value> {
        let mut fields = Map::new();
        fields.insert(
            "approvalPolicy".to_owned(),
            json!(self.mode.approval_policy()),
        );
        fields.insert(
            "approvalsReviewer".to_owned(),
            json!(self.mode.approvals_reviewer()),
        );
        fields.insert("sandboxPolicy".to_owned(), self.mode.sandbox_policy());
        let fast = self.fast && self.current().is_some_and(Model::fast);
        fields.insert("serviceTier".to_owned(), json!(fast.then_some("fast")));

        if !self.model.is_empty() {
            fields.insert("model".to_owned(), json!(self.model));
            fields.insert("effort".to_owned(), json!(self.effort));
            fields.insert(
                "collaborationMode".to_owned(),
                json!({
                    "mode": self.collaboration.as_ref(),
                    "settings": {
                        "model": self.model,
                        "reasoning_effort": self.effort,
                        "developer_instructions": null,
                    },
                }),
            );
        }

        fields
    }
}

fn select(
    id: &str,
    name: &str,
    description: &str,
    category: &str,
    current: &str,
    options: Vec<Value>,
) -> Value {
    json!({
        "id": id,
        "name": name,
        "description": description,
        "category": category,
        "type": "select",
        "currentValue": current,
        "options": options,
    })
}

/// A model's name as Codex's adapter shows it: `gpt-6.2-sol` as `6.2 Sol`.
fn display_name(name: &str) -> String {
    let Some(rest) = name
        .get(..4)
        .filter(|gpt| gpt.eq_ignore_ascii_case("gpt-"))
        .map(|_| &name[4..])
    else {
        return name.to_owned();
    };
    rest.split(['-', '/'])
        .filter(|part| !part.is_empty())
        .map(capitalized)
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
