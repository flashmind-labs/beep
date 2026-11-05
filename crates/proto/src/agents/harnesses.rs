//! Coding-agent harnesses: custom agents, accounts, models and credentials.

use crate::{
    ApiError, SecretText,
    node::{EnvSecret, harness::harness},
};
use std::{collections::HashSet, time::SystemTime};

wire_structs! {
    /// One machine's settings for a harness (`ListHarnessSettings`).
    pub struct MachineHarnessSettings {
        pub machine_id: String,
        pub machine_name: String,
        pub settings: HarnessSettings,
    }

    /// One custom agent a coding agent harness defines, as
    /// `ListCustomAgents` reads it: a named prompt with a description, and
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

/// The most of a custom agent's prompt `ListCustomAgents` gives.
pub const MAX_CUSTOM_AGENT_PROMPT: usize = 16 * 1024;

/// The most custom agents one `ListCustomAgents` gives.
pub const MAX_CUSTOM_AGENTS: usize = 200;

wire_enum! {
    /// The coding agent harness that defines a `CustomAgent`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CustomAgentHarness {
        0 Claude {}
        1 OpenCode {}
        2 Codex {}
        3 Pi {}
        4 Flsh {}
    }
}

impl CustomAgentHarness {
    /// The harness's id, as the apps name it elsewhere: `claude`,
    /// `opencode`, `codex`, `pi` or `flsh`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude {} => "claude",
            Self::OpenCode {} => "opencode",
            Self::Codex {} => "codex",
            Self::Pi {} => "pi",
            Self::Flsh {} => "flsh",
        }
    }
}

wire_enum! {
    /// Where a `CustomAgent` is defined: `Project` in the project folder,
    /// so it travels with the repository, `User` in the user's own harness
    /// folder on the machine.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CustomAgentScope {
        0 Project {}
        1 User {}
    }
}

impl CustomAgentScope {
    /// `project` or `user`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project {} => "project",
            Self::User {} => "user",
        }
    }
}

wire_structs! {
    /// The coding agent harnesses on one machine, as `ListAgentHarnesses`
    /// gives them.
    pub struct AgentHarnesses {
        pub machine_id: String,
        pub machine_name: String,
        /// When the machine last looked the newest versions up; `None` when it
        /// never did.
        pub checked_at: Option<SystemTime>,
        /// Every harness Beep knows, installed or not, then any other the
        /// machine runs.
        pub harnesses: Vec<AgentHarness>,
    }

    /// One harness on a machine.
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
}

ffi_enum! {
    /// How a harness signs in on a machine. `BeepToken` is the token Beep
    /// keeps for it (Agent accounts), handed to every session it starts.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum HarnessSignIn {
        0 Unknown {}
        1 SignedIn { account: Option<String>, plan: Option<String> }
        2 SignedOut {}
        3 BeepToken { last4: String }
    }
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
        pub env: Vec<HarnessEnv>,
    }

    /// An env var a harness gets. The server never sends a value back: in
    /// replies `value` is always `None`. Not for the phone app, which has no
    /// `SecretText` type.
    pub struct HarnessEnv {
        pub name: String,
        pub value: Option<SecretText>,
    }
}

ffi_structs! {
    /// How the user's coding agents sign in, as `GetAgentAccounts` gives it.
    pub struct AgentAccounts {
        /// The Claude Code token the machines get, when there is one.
        pub claude: Option<AgentToken>,
        /// The API key Codex uses instead of each machine's own sign-in.
        pub codex_key: Option<AgentToken>,
        /// The caller's machines with Claude Code or Codex, by name.
        pub machines: Vec<AgentMachine>,
    }

    /// A token Beep keeps for a coding agent. The secret stays on the server.
    pub struct AgentToken {
        /// The secret's last four characters.
        pub last4: String,
        pub created_at: SystemTime,
        /// The server can no longer open it; the user has to sign in again.
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

ffi_enum! {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::IntoStaticStr, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum AgentAccount {
    Claude,
    Codex,
}

impl AgentAccount {
    /// The slug the wire and the database carry, such as `claude`.
    pub fn slug(self) -> &'static str {
        self.into()
    }

    /// The env var the agent reads its token from: Claude Code's
    /// long-lived OAuth token, or the API key Codex takes in place of a
    /// sign-in.
    pub fn env_var(self) -> &'static str {
        match self {
            Self::Claude => "CLAUDE_CODE_OAUTH_TOKEN",
            Self::Codex => "CODEX_API_KEY",
        }
    }
}

ffi_structs! {
    /// The models a coding agent can run on a machine, as `ListAgentModels`
    /// gives them.
    pub struct AgentModels {
        pub pick: AgentModelPick,
        /// The model the agent runs when none is picked, when it says.
        pub current: Option<String>,
        /// The models to pick from, for `AgentModelPick::Listed`.
        pub models: Vec<AgentModel>,
    }
}

ffi_enum! {
    /// How the start form picks a coding agent's model: `Fixed` when none
    /// can be picked (the agent runs its own, or the machine's beep-node is
    /// too old to pick one), `Listed` from `AgentModels::models`, `Named`
    /// by any name the agent takes when it lists none.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    pub enum AgentModelPick {
        0 Fixed {}
        1 Listed {}
        2 Named {}
    }
}

ffi_enum! {
    /// What a coding agent starts on, as `StartAgentSession::checkout`
    /// asks: a new `beep/<slug>` branch in the checkout itself, or one in a
    /// new worktree at `.worktrees/beep-<slug>`, made from `base` (the
    /// checkout's `HEAD` when `None`). `NamedWorktree` is `NewWorktree` on
    /// a branch named `branch` rather than from the prompt.
    /// `BranchWorktree` starts on the existing branch `branch`: in the
    /// worktree that has it checked out, else in a new one at
    /// `.worktrees/<branch>` after fetching it from `origin`. A server
    /// older than `Feature::BRANCH_WORKTREES` refuses both, and so does a
    /// node from before them. See `node::agent::CheckoutPlan`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum AgentCheckoutPlan {
        0 NewBranch { base: Option<String> }
        1 NewWorktree { base: Option<String> }
        2 NamedWorktree { base: Option<String>, branch: String } since BRANCH_WORKTREES
        3 BranchWorktree { branch: String } since BRANCH_WORKTREES
    }
}

ffi_enum! {
    /// How hard a coding agent thinks, as `StartAgentSession::effort`
    /// asks. Which levels an agent takes is `node::agent::efforts`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    pub enum AgentEffort {
        0 Off {}
        1 Minimal {}
        2 Low {}
        3 Medium {}
        4 High {}
        5 Xhigh {}
        6 Max {}
    }
}

ffi_structs! {
    /// One model a coding agent offers.
    #[derive(Eq)]
    pub struct AgentModel {
        /// What `StartAgentSession::model` names it by.
        pub id: String,
        pub name: String,
        pub description: Option<String>,
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

    /// The agent files on one machine, as `ReadAgentFiles` reads them.
    pub struct MachineAgentFiles {
        pub machine_id: String,
        pub machine_name: String,
        pub files: Vec<AgentFile>,
        /// Why the machine could not be read or written; `files` is then empty.
        pub error: Option<String>,
    }

    /// One coding agent's instructions file on a machine.
    pub struct AgentFile {
        /// `claude`, `codex`, `gemini` or `pi`.
        pub agent: String,
        /// Under the home folder, such as `~/.claude/CLAUDE.md`.
        pub path: String,
        /// Its text; `None` when the agent's folder is there but the file is
        /// not.
        pub text: Option<String>,
        /// It holds the newest instructions between Beep's markers.
        pub synced: bool,
    }

    /// A credential as `ListCredentials` lists it. The secret stays on the
    /// server.
    pub struct Credential {
        pub id: String,
        pub kind: CredentialKind,
        /// The secret's last four characters.
        pub last4: String,
        /// Who the token signs in as, such as a GitHub login. `None` for API
        /// keys, and for a Forgejo server the server could not reach to check.
        pub account: Option<String>,
        /// The env var the machines get it as, such as `GH_TOKEN`. Empty for
        /// Forgejo, whose tokens go into teacli's config instead.
        pub env_var: String,
        /// Whether the caller's machines get it.
        pub to_machines: bool,
        pub created_at: SystemTime,
        /// The server can no longer open it; the user has to enter it again.
        /// Machines do not get it.
        pub unreadable: bool,
    }
}

ffi_enum! {
    /// What `CheckCredential` found. `machine` is the name of the machine
    /// that reached the server, `None` when Beep's server did. `message`
    /// says what happened, for the user.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum CredentialCheck {
        0 Valid { account: String, machine: Option<String> }
        1 Refused { machine: Option<String>, message: String }
        2 Unreachable { message: String }
    }
}

ffi_enum! {
    /// What a credential is for. `url` is a Forgejo or Gitea server's base
    /// URL, such as `https://git.example.com`; `provider` an `ApiProvider`
    /// slug, such as `openrouter`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum CredentialKind {
        0 GitHub {}
        1 Forgejo { url: String }
        2 ApiKey { provider: String }
    }
}

/// The providers an API key can be saved for. On the wire a provider is its
/// slug, so a newer server can add one without changing a reply's shape.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    strum::IntoStaticStr,
    strum::EnumString,
    strum::EnumIter,
)]
#[strum(serialize_all = "lowercase")]
pub enum ApiProvider {
    OpenRouter,
    ElevenLabs,
    OpenAi,
    /// Dictation.
    Mistral,
    Anthropic,
    Gemini,
    DeepSeek,
    Moonshot,
    Zai,
    /// Images and videos.
    Venice,
    /// 3D models.
    Meshy,
    /// Web search and reading pages, for the assistant.
    Firecrawl,
}

impl ApiProvider {
    /// Every provider, in the order Settings lists them.
    pub fn all() -> impl Iterator<Item = Self> {
        <Self as strum::IntoEnumIterator>::iter()
    }

    /// The slug the wire and the database carry, such as `openrouter`.
    pub fn slug(self) -> &'static str {
        self.into()
    }

    /// What the user knows it as.
    pub fn name(self) -> &'static str {
        match self {
            Self::OpenRouter => "OpenRouter",
            Self::ElevenLabs => "ElevenLabs",
            Self::OpenAi => "OpenAI",
            Self::Mistral => "Mistral",
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Gemini",
            Self::DeepSeek => "DeepSeek",
            Self::Moonshot => "Moonshot",
            Self::Zai => "Z.ai",
            Self::Venice => "Venice",
            Self::Meshy => "Meshy",
            Self::Firecrawl => "Firecrawl",
        }
    }

    /// The env var tools read the key from.
    pub fn env_var(self) -> &'static str {
        match self {
            Self::OpenRouter => "OPENROUTER_API_KEY",
            Self::ElevenLabs => "ELEVENLABS_API_KEY",
            Self::OpenAi => "OPENAI_API_KEY",
            Self::Mistral => "MISTRAL_API_KEY",
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::Gemini => "GEMINI_API_KEY",
            Self::DeepSeek => "DEEPSEEK_API_KEY",
            Self::Moonshot => "MOONSHOT_API_KEY",
            Self::Zai => "ZAI_API_KEY",
            Self::Venice => "VENICE_API_KEY",
            Self::Meshy => "MESHY_API_KEY",
            Self::Firecrawl => "FIRECRAWL_API_KEY",
        }
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
