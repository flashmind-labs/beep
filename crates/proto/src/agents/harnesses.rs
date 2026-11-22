//! Coding-agent harnesses: custom agents, accounts, models and credentials.

use crate::{
    ApiError, SecretText,
    node::{EnvSecret, harness::harness},
};
use std::{collections::HashSet, str::FromStr, time::SystemTime};

/// One machine's settings for a harness.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MachineHarnessSettings {
    pub machine_id: String,
    pub machine_name: String,
    pub settings: HarnessSettings,
}

wire_structs! {
    /// One custom agent a coding agent harness defines: a named prompt with a description, and
    /// optionally the model, tools and color it runs with.
    #[derive(Eq)]
    pub struct CustomAgent {
        /// The name the harness knows it by; the file's stem when it names
        /// none.
        pub name: String,
        pub harness: CustomAgentHarness,
        pub scope: CustomAgentScope,
        pub description: String,
        /// As the file names it, such as `opus` or `inherit`.
        pub model: Option<String>,
        /// The tools it may use, as the file lists them; empty for all.
        pub tools: Vec<String>,
        /// As the file names it, such as `blue` or `#3b82f6`.
        pub color: Option<String>,
        /// The file it came from: relative to the project folder for a
        /// project agent, starting `~/` for a user one.
        pub path: String,
        /// Its prompt, at most [`MAX_CUSTOM_AGENT_PROMPT`] bytes.
        pub prompt: String,
        /// The prompt goes on past what `prompt` holds.
        pub prompt_cut: bool,
        /// Why the file could not be read, when it could not; the other fields
        /// then say what was found.
        pub error: Option<String>,
    }
}

/// The most of a custom agent's prompt a node gives.
pub const MAX_CUSTOM_AGENT_PROMPT: usize = 16 * 1024;

/// The most custom agents a node gives.
pub const MAX_CUSTOM_AGENTS: usize = 200;

wire_enum! {
    /// The coding agent harness that defines a `CustomAgent`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[derive(strum::AsRefStr, strum::IntoStaticStr)]
    pub enum CustomAgentHarness {
        #[strum(serialize = "claude")]
        0 Claude {}
        #[strum(serialize = "opencode")]
        1 OpenCode {}
        #[strum(serialize = "codex")]
        2 Codex {}
        #[strum(serialize = "pi")]
        3 Pi {}
        #[strum(serialize = "flsh")]
        4 Flsh {}
    }
}

wire_enum! {
    /// Where a `CustomAgent` is defined: `Project` in the project folder,
    /// so it travels with the repository, `User` in the user's own harness
    /// folder on the machine.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[derive(strum::AsRefStr, strum::IntoStaticStr)]
    pub enum CustomAgentScope {
        #[strum(serialize = "project")]
        0 Project {}
        #[strum(serialize = "user")]
        1 User {}
    }
}

/// The coding agent harnesses on one machine.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AgentHarnesses {
    pub machine_id: String,
    pub machine_name: String,
    /// When the machine last looked the newest versions up; `None` when it
    /// never did.
    #[serde(with = "crate::projects::epoch_ms::option")]
    pub checked_at: Option<SystemTime>,
    /// Every harness Beep knows, installed or not, then any other the
    /// machine runs.
    pub harnesses: Vec<AgentHarness>,
}

/// One harness on a machine.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AgentHarness {
    /// Its slug, such as `claude`.
    pub agent: String,
    /// Its CLI is on the machine.
    pub installed: bool,
    /// The machine can start it; only these are offered.
    pub runnable: bool,
    /// Its version, such as `2.1.312`.
    pub version: Option<String>,
    /// The newest version, when it could be looked up.
    pub latest: Option<String>,
    /// What the machine starts for it when no binary path is set.
    pub program: Option<String>,
    /// The variable its home folder is set with, such as `CODEX_HOME`.
    pub home_var: Option<String>,
    /// Its home folder when that variable is not set, such as `~/.codex`.
    pub home_default: Option<String>,
    pub sign_in: HarnessSignIn,
    /// The command Install or Update runs there; `None` when there is none.
    pub install: Option<String>,
    pub settings: HarnessSettings,
}

/// How a harness signs in on a machine. `BeepToken` is the token Beep
/// keeps for it (Agent accounts), handed to every session it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessSignIn {
    Unknown {},
    SignedIn {
        account: Option<String>,
        plan: Option<String>,
    },
    SignedOut {},
    BeepToken {
        last4: String,
    },
}

impl serde::Serialize for HarnessSignIn {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let (state, account, plan, last4) = match self {
            Self::Unknown {} => ("unknown", None, None, None),
            Self::SignedOut {} => ("signed_out", None, None, None),
            Self::SignedIn { account, plan } => {
                ("signed_in", account.as_deref(), plan.as_deref(), None)
            }
            Self::BeepToken { last4 } => ("beep_token", None, None, Some(last4.as_str())),
        };
        let mut out = s.serialize_struct("HarnessSignIn", 4)?;
        out.serialize_field("state", state)?;
        out.serialize_field("account", &account)?;
        out.serialize_field("plan", &plan)?;
        out.serialize_field("last4", &last4)?;
        out.end()
    }
}

fn serialize_env_names<S: serde::Serializer>(env: &[HarnessEnv], s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;

    let mut seq = s.serialize_seq(Some(env.len()))?;
    for var in env {
        seq.serialize_element(&var.name)?;
    }
    seq.end()
}

wire_structs! {
    /// The caller's settings for one harness on one machine. Empty text means
    /// what the machine finds.
    pub struct HarnessSettings {
        /// Off: not offered on that machine.
        pub enabled: bool,
        /// The program to start in place of the one the machine finds.
        pub program: String,
        /// Its home folder, set with its home variable.
        pub home: String,
        /// Added to every session it starts.
        pub args: Vec<String>,
        #[serde(serialize_with = "serialize_env_names")]
        pub env: Vec<HarnessEnv>,
    }

    /// An env var a harness gets. In replies `value` is always `None`. Not
    /// for the phone app, which has no `SecretText` type.
    pub struct HarnessEnv {
        pub name: String,
        pub value: Option<SecretText>,
    }
}

wire_structs! {
    /// How the user's coding agents sign in.
    pub struct AgentAccounts {
        /// The Claude Code token the machines get, when there is one.
        pub claude: Option<AgentToken>,
        /// The API key Codex uses instead of each machine's own sign-in.
        pub codex_key: Option<AgentToken>,
        /// The caller's machines with Claude Code or Codex, by name.
        pub machines: Vec<AgentMachine>,
        /// The named accounts, by agent then name.
        #[serde(default)]
        pub named: Vec<NamedAgentToken>,
    }

    /// A named account's token.
    pub struct NamedAgentToken {
        pub agent: AgentAccount,
        pub name: AccountName,
        /// The env var it goes in, such as `CODEX_ACCESS_TOKEN`.
        pub env_var: String,
        pub token: AgentToken,
    }

    /// A token Beep keeps for a coding agent.
    pub struct AgentToken {
        /// The secret's last four characters.
        pub last4: String,
        pub created_at: SystemTime,
        /// Beep can no longer open it; the user has to sign in again.
        pub unreadable: bool,
    }

    /// One of the user's machines with Claude Code or Codex.
    pub struct AgentMachine {
        pub machine_id: String,
        pub machine_name: String,
        pub online: bool,
        /// Claude Code is on it, as it last said.
        pub claude: bool,
        /// Codex is on it, as it last said.
        pub codex: bool,
        /// Claude Code sessions Beep runs on it now.
        pub claude_sessions: u32,
        /// Whether Codex is signed in there; `None` when not known, such as
        /// while it is offline.
        pub codex_signed_in: Option<bool>,
    }

    /// A Codex device sign-in under way on a machine.
    pub struct CodexLogin {
        /// The node's background task running `codex login`.
        pub task: u32,
        /// The page to enter the code on.
        pub url: String,
        pub code: String,
        /// When the code stops working.
        pub expires_at: SystemTime,
    }
}

wire_enum! {
    /// How a Codex device sign-in stands. `Gone` is no longer on its
    /// machine, such as after a restart, or the machine is offline.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum CodexLoginState {
        0 Waiting {}
        1 SignedIn {}
        2 Failed { message: String }
        3 Gone {}
    }
}

/// The coding agents Beep can keep a token for. On the wire an agent is
/// its slug, the name the node gives the agent.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    strum::IntoStaticStr,
    strum::AsRefStr,
    strum::EnumString,
    serde::Serialize,
    serde::Deserialize,
)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum AgentAccount {
    Claude,
    Codex,
}

impl AgentAccount {
    /// The env var the agent reads its token from: Claude Code's
    /// long-lived OAuth token, or the API key Codex takes in place of a
    /// sign-in.
    pub fn env_var(self) -> &'static str {
        match self {
            Self::Claude => "CLAUDE_CODE_OAUTH_TOKEN",
            Self::Codex => "CODEX_API_KEY",
        }
    }

    /// The env var a named account's token goes in. Codex's is a ChatGPT
    /// personal access token, which wins over the machine's own sign-in; an
    /// API key in `CODEX_API_KEY` does not.
    pub fn named_env_var(self) -> &'static str {
        match self {
            Self::Claude => "CLAUDE_CODE_OAUTH_TOKEN",
            Self::Codex => "CODEX_ACCESS_TOKEN",
        }
    }
}

/// The name the user gave one of their accounts, such as `Work`. A start
/// that names one runs signed in as that account of its agent; the same
/// name across agents is one identity, so a Claude Code session on `Work`
/// starts its Codex helpers on Codex's `Work`. The unnamed account is the
/// default one Agent accounts kept before names.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct AccountName(String);

/// The longest account name taken, in characters.
const MAX_ACCOUNT_NAME: usize = 32;

impl FromStr for AccountName {
    type Err = ApiError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let name = text.trim();

        if name.is_empty() {
            return Err(ApiError::bad_request("Name the account first."));
        }

        if name.chars().count() > MAX_ACCOUNT_NAME {
            return Err(ApiError::bad_request(format!(
                "An account name has at most {MAX_ACCOUNT_NAME} characters."
            )));
        }

        if name.chars().any(|c| c.is_control() || c == '~' || c == '/') {
            return Err(ApiError::bad_request(
                "An account name cannot hold ~, / or control characters.",
            ));
        }

        if name.eq_ignore_ascii_case("default") {
            return Err(ApiError::bad_request(
                "Default is the account without a name; pick another name.",
            ));
        }

        Ok(Self(name.to_owned()))
    }
}

impl AsRef<str> for AccountName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AccountName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The models a coding agent can run on a machine, as `ListAgentModels`
/// gives them.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AgentModels {
    pub pick: AgentModelPick,
    /// The model the agent runs when none is picked, when it says.
    pub current: Option<String>,
    /// The models to pick from, for `AgentModelPick::Listed`.
    pub models: Vec<AgentModel>,
}

/// How the start form picks a coding agent's model: `Fixed` when none
/// can be picked (the agent runs its own, or the machine's beep-node is
/// too old to pick one), `Listed` from `AgentModels::models`, `Named`
/// by any name the agent takes when it lists none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum AgentModelPick {
    Fixed {},
    Listed {},
    Named {},
}

impl serde::Serialize for AgentModelPick {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(Into::<&'static str>::into(*self))
    }
}

wire_structs! {
    /// One model a coding agent offers.
    #[derive(Eq)]
    pub struct AgentModel {
        /// What `StartAgentSession::model` names it by.
        pub id: String,
        pub name: String,
        pub description: Option<String>,
        /// The harness's default usable context, when reported.
        #[serde(default)]
        pub context_window: Option<u64>,
        /// The largest usable window the harness reports for this model.
        #[serde(default)]
        pub max_context_window: Option<u64>,
        /// Who serves it, such as `openrouter`, when the id names one;
        /// `name` leaves it out.
        #[serde(default)]
        pub provider: Option<String>,
    }

    /// One saved version of the user's instructions.
    pub struct InstructionsVersion {
        /// 1 for the first save, then one more each time.
        pub version: u32,
        /// Markdown.
        pub text: String,
        /// What made it, such as "Restored version 3"; empty for an edit.
        pub note: String,
        pub saved_at: SystemTime,
    }
}

wire_enum! {
    /// What checking a credential found. `machine` is the name of the machine
    /// that reached the host, `None` when the desktop did. `message`
    /// says what happened, for the user.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum CredentialCheck {
        0 Valid { account: String, machine: Option<String> }
        1 Refused { machine: Option<String>, message: String }
        2 Unreachable { message: String }
    }
}

// What a harness's settings may hold.
/// The longest program path or home folder taken.
const MAX_PATH: usize = 1024;
/// The most launch arguments, and the longest one.
const MAX_ARGS: usize = 32;
const MAX_ARG: usize = 1024;
/// The most env vars, the longest name and the longest value.
const MAX_ENV: usize = 32;
const MAX_ENV_NAME: usize = 128;
const MAX_ENV_VALUE: usize = 8 * 1024;
/// The longest agent slug, for one a machine's `node.toml` adds.
const MAX_AGENT: usize = 32;

/// A slug Beep knows, or one a machine's `node.toml` could add.
pub fn check_agent(agent: &str) -> Result<(), ApiError> {
    let plain = !agent.is_empty()
        && agent.len() <= MAX_AGENT
        && agent
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if harness(agent).is_some() || plain {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "{agent:?} is not a coding agent."
        )))
    }
}

/// `settings` trimmed, or why they cannot be saved.
pub fn check_settings(settings: HarnessSettings) -> Result<HarnessSettings, ApiError> {
    let line = |what: &str, text: &str, max: usize| -> Result<String, ApiError> {
        let text = text.trim();

        if text.len() > max {
            return Err(ApiError::bad_request(format!("The {what} is too long.")));
        }

        if text.chars().any(char::is_control) {
            return Err(ApiError::bad_request(format!(
                "The {what} cannot hold a line break or control character."
            )));
        }

        Ok(text.to_owned())
    };
    let program = line("binary path", &settings.program, MAX_PATH)?;
    let home = line("home folder", &settings.home, MAX_PATH)?;

    if settings.args.len() > MAX_ARGS {
        return Err(ApiError::bad_request(format!(
            "At most {MAX_ARGS} launch arguments."
        )));
    }

    let args = settings
        .args
        .iter()
        .map(|arg| line("launch argument", arg, MAX_ARG))
        .filter(|arg| !matches!(arg, Ok(arg) if arg.is_empty()))
        .collect::<Result<Vec<String>, ApiError>>()?;
    if settings.env.len() > MAX_ENV {
        return Err(ApiError::bad_request(format!(
            "At most {MAX_ENV} environment variables."
        )));
    }

    let mut seen = HashSet::new();
    let mut env = Vec::new();

    for var in settings.env {
        let name = var.name.trim().to_owned();

        if name.is_empty() && var.value.is_none() {
            continue;
        }

        let valid = name.len() <= MAX_ENV_NAME
            && name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err(ApiError::bad_request(format!(
                "{name:?} is not an environment variable name."
            )));
        }

        if !seen.insert(name.clone()) {
            return Err(ApiError::bad_request(format!("{name} is set twice.")));
        }

        if let Some(value) = &var.value
            && (value.expose().len() > MAX_ENV_VALUE || value.expose().contains('\0'))
        {
            return Err(ApiError::bad_request(format!(
                "The value of {name} is too long or not text."
            )));
        }

        env.push(HarnessEnv {
            name,
            value: var.value,
        });
    }

    Ok(HarnessSettings {
        enabled: settings.enabled,
        program,
        home,
        args,
        env,
    })
}

/// Each of `wanted` with its new value, or `kept`'s for that name; one with neither is dropped.
pub fn merged_env(kept: Vec<EnvSecret>, wanted: Vec<HarnessEnv>) -> Vec<EnvSecret> {
    wanted
        .into_iter()
        .filter_map(|var| {
            let value = var.value.or_else(|| {
                kept.iter()
                    .find(|old| old.name == var.name)
                    .map(|old| old.value.clone())
            })?;
            Some(EnvSecret {
                name: var.name,
                value,
            })
        })
        .collect()
}

/// The env var `gh` reads its token from.
pub const GITHUB_ENV_VAR: &str = "GH_TOKEN";
