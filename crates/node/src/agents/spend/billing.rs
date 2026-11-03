//! Whether what a session spent was paid by the token or covered by a
//! subscription, as far as this machine tells without reading a secret.
//!
//! - Claude Code: a message id or model of Bedrock or Vertex in the log is
//!   metered. Otherwise its settings (the user's, then the project's) are
//!   read for which keys they set, never their values: an API key, an auth
//!   token, an `apiKeyHelper` or a Bedrock, Vertex or Foundry switch is
//!   metered, `forceLoginMethod` says which login. `.credentials.json` is
//!   never read.
//! - Codex: a token count carrying a ChatGPT plan is a subscription, a
//!   provider other than `openai` is metered; otherwise `auth.json`'s
//!   `auth_mode`, the only field read of it.
//! - pi and OpenCode: each request's provider, by the `type` their
//!   `auth.json` gives it (OAuth is a subscription), else by name.
//! - flsh: the provider its model names, by name.
//!
//! Whatever is left unknown the server counts as a subscription for Claude
//! Code and Codex.

use crate::found::Agent;
use beep_proto::node::usage::Billing;
use serde::{Deserialize, de::IgnoredAny};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

/// Providers that ride a subscription, as pi, OpenCode and flsh name them.
const SUBSCRIPTION_PROVIDERS: &[&str] = &[
    "github-copilot",
    "github-copilot-enterprise",
    "copilot",
    "openai-codex",
    "codex",
    "chatgpt",
    "claude-code",
    "claude-max",
    "claude-pro",
    "google-gemini-cli",
    "google-antigravity",
    "kimi-code",
    "kimi-for-coding",
    "zai-coding-plan",
];

/// Settings env keys that send Claude Code through a key or a cloud.
const CLAUDE_METERED_ENV: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
];
/// The settings env key of a Claude account's long-lived token.
const CLAUDE_ACCOUNT_ENV: &str = "CLAUDE_CODE_OAUTH_TOKEN";

/// The largest settings or auth file read.
const MAX_FILE: u64 = 1024 * 1024;
/// The most project folders whose Claude Code settings are kept per pass.
const MAX_PROJECTS: usize = 1_000;

/// Where the files that tell how each agent signs in are.
#[derive(Debug, Clone, Default)]
pub(crate) struct AccountFiles {
    /// Claude Code's own folder, with `settings.json`.
    pub(crate) claude: Option<PathBuf>,
    /// Codex's `auth.json`.
    pub(crate) codex: Option<PathBuf>,
    /// pi's `auth.json`.
    pub(crate) pi: Option<PathBuf>,
    /// OpenCode's `auth.json`.
    pub(crate) opencode: Option<PathBuf>,
}

/// How each agent signs in here, read again each pass.
#[derive(Debug, Default)]
pub(super) struct Accounts {
    /// From Claude Code's user settings.
    claude: Billing,
    /// From each project's Claude Code settings, by folder, as asked.
    claude_projects: HashMap<String, Billing>,
    /// From Codex's `auth_mode`.
    codex: Billing,
    /// By provider, from `auth.json`.
    pi: BTreeMap<String, Billing>,
    opencode: BTreeMap<String, Billing>,
}

/// The keys of Claude Code's settings that tell how it signs in.
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ClaudeSettings {
    /// Only the names are kept.
    env: BTreeMap<String, IgnoredAny>,
    api_key_helper: Option<IgnoredAny>,
    /// `claudeai` or `console`.
    force_login_method: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CodexAuth {
    auth_mode: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ProviderAuth {
    #[serde(rename = "type")]
    kind: Option<String>,
}

/// `path` parsed, when it is there, small and well formed. Nothing of it
/// is logged: an auth file holds tokens.
fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(value) => Some(value),
        Err(_) => {
            log::debug!("sign-in file not read; path={}", path.display());
            None
        }
    }
}

fn claude_settings(path: &Path) -> Billing {
    let Some(settings) = read::<ClaudeSettings>(path) else {
        return Billing::Unknown;
    };
    let keyed = CLAUDE_METERED_ENV
        .iter()
        .any(|key| settings.env.contains_key(*key));
    if keyed || settings.api_key_helper.is_some() {
        return Billing::Metered;
    }
    match settings.force_login_method.as_deref() {
        Some("console") => Billing::Metered,
        Some("claudeai") => Billing::Subscription,
        _ if settings.env.contains_key(CLAUDE_ACCOUNT_ENV) => Billing::Subscription,
        _ => Billing::Unknown,
    }
}

/// What a provider's `auth.json` entry type means.
fn auth_kind(kind: Option<&str>) -> Billing {
    match kind {
        Some("oauth") => Billing::Subscription,
        Some(_) => Billing::Metered,
        None => Billing::Unknown,
    }
}

fn provider_auth(path: &Path) -> BTreeMap<String, Billing> {
    read::<BTreeMap<String, ProviderAuth>>(path)
        .unwrap_or_default()
        .into_iter()
        .map(|(provider, auth)| (provider, auth_kind(auth.kind.as_deref())))
        .filter(|(_, billing)| *billing != Billing::Unknown)
        .collect()
}

/// What a provider named `provider` is, by name alone.
fn provider_by_name(provider: &str) -> Billing {
    if provider.is_empty() {
        Billing::Unknown
    } else if SUBSCRIPTION_PROVIDERS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(provider))
    {
        Billing::Subscription
    } else {
        Billing::Metered
    }
}

/// Whether a Claude Code request went through Bedrock or Vertex, by its
/// message id (`msg_bdrk_`, `msg_vrtx_`) or model (`us.anthropic.claude-`,
/// `claude-opus-4@20250514`).
pub(super) fn claude_cloud(message_id: Option<&str>, model: &str) -> bool {
    let cloud_id =
        message_id.is_some_and(|id| id.starts_with("msg_bdrk_") || id.starts_with("msg_vrtx_"));
    cloud_id || model.contains("anthropic.") || model.contains('@')
}

/// The provider flsh names in a model such as `openrouter:example/model`.
pub(super) fn flsh_provider(model: &str) -> Option<&str> {
    model.split_once(':').map(|(provider, _)| provider)
}

impl Accounts {
    pub(super) fn read(files: &AccountFiles) -> Self {
        let codex = files
            .codex
            .as_deref()
            .and_then(read::<CodexAuth>)
            .and_then(|auth| auth.auth_mode)
            .map_or(Billing::Unknown, |mode| {
                let mode = mode.to_ascii_lowercase();
                if mode.starts_with("chatgpt") {
                    Billing::Subscription
                } else if mode.contains("api") {
                    Billing::Metered
                } else {
                    Billing::Unknown
                }
            });
        Self {
            claude: files.claude.as_deref().map_or(Billing::Unknown, |dir| {
                claude_settings(&dir.join("settings.json"))
            }),
            claude_projects: HashMap::new(),
            codex,
            pi: files.pi.as_deref().map(provider_auth).unwrap_or_default(),
            opencode: files
                .opencode
                .as_deref()
                .map(provider_auth)
                .unwrap_or_default(),
        }
    }

    /// How a request of `agent` was paid for: `seen` when its log said,
    /// else from its provider and folder and how the agent signs in here.
    pub(super) fn resolve(
        &mut self,
        agent: Agent,
        seen: Billing,
        provider: Option<&str>,
        cwd: Option<&str>,
    ) -> Billing {
        if seen != Billing::Unknown {
            return seen;
        }
        match agent {
            Agent::Claude => self.claude(cwd),
            Agent::Codex => match provider {
                Some(provider) if !provider.eq_ignore_ascii_case("openai") => Billing::Metered,
                _ => self.codex,
            },
            Agent::Pi => self.provider(&self.pi, provider),
            Agent::OpenCode => self.provider(&self.opencode, provider),
            Agent::Flsh => provider.map_or(Billing::Unknown, provider_by_name),
        }
    }

    fn provider(&self, auth: &BTreeMap<String, Billing>, provider: Option<&str>) -> Billing {
        let Some(provider) = provider else {
            return Billing::Unknown;
        };
        auth.get(provider)
            .copied()
            .unwrap_or_else(|| provider_by_name(provider))
    }

    /// Claude Code in `cwd`: the project's local settings, then its shared
    /// ones, then the user's, the first that tells.
    fn claude(&mut self, cwd: Option<&str>) -> Billing {
        let Some(cwd) = cwd.filter(|cwd| Path::new(cwd).is_absolute()) else {
            return self.claude;
        };
        if let Some(billing) = self.claude_projects.get(cwd) {
            return *billing;
        }
        let dir = Path::new(cwd).join(".claude");
        let project = [
            claude_settings(&dir.join("settings.local.json")),
            claude_settings(&dir.join("settings.json")),
        ]
        .into_iter()
        .find(|billing| *billing != Billing::Unknown)
        .unwrap_or(self.claude);
        if self.claude_projects.len() < MAX_PROJECTS {
            self.claude_projects.insert(cwd.to_owned(), project);
        }
        project
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_settings_tell_by_their_keys() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let user = dir.path().join("user");
        fs::create_dir_all(&user).expect("a folder");
        let files = AccountFiles {
            claude: Some(user.clone()),
            ..AccountFiles::default()
        };
        assert_eq!(
            Accounts::read(&files).resolve(Agent::Claude, Billing::Unknown, None, None),
            Billing::Unknown
        );
        fs::write(
            user.join("settings.json"),
            r#"{"env":{"ANTHROPIC_API_KEY":"example-value"}}"#,
        )
        .expect("settings");
        let mut accounts = Accounts::read(&files);
        assert_eq!(
            accounts.resolve(Agent::Claude, Billing::Unknown, None, None),
            Billing::Metered
        );
        let project = dir.path().join("project");
        fs::create_dir_all(project.join(".claude")).expect("a folder");
        fs::write(
            project.join(".claude/settings.local.json"),
            r#"{"forceLoginMethod":"claudeai"}"#,
        )
        .expect("settings");
        let cwd = project.to_str().expect("a path");
        assert_eq!(
            accounts.resolve(Agent::Claude, Billing::Unknown, None, Some(cwd)),
            Billing::Subscription
        );
        assert_eq!(
            accounts.resolve(Agent::Claude, Billing::Metered, None, Some(cwd)),
            Billing::Metered,
            "what the log said wins"
        );
    }

    #[test]
    fn codex_reads_its_auth_mode_and_provider() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let auth = dir.path().join("auth.json");
        fs::write(
            &auth,
            r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":"x"}}"#,
        )
        .expect("auth");
        let mut accounts = Accounts::read(&AccountFiles {
            codex: Some(auth.clone()),
            ..AccountFiles::default()
        });
        assert_eq!(
            accounts.resolve(Agent::Codex, Billing::Unknown, Some("openai"), None),
            Billing::Subscription
        );
        assert_eq!(
            accounts.resolve(Agent::Codex, Billing::Unknown, Some("openrouter"), None),
            Billing::Metered
        );
        fs::write(&auth, r#"{"auth_mode":"apikey"}"#).expect("auth");
        let mut accounts = Accounts::read(&AccountFiles {
            codex: Some(auth),
            ..AccountFiles::default()
        });
        assert_eq!(
            accounts.resolve(Agent::Codex, Billing::Unknown, None, None),
            Billing::Metered
        );
    }

    #[test]
    fn providers_go_by_their_auth_type_then_their_name() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let auth = dir.path().join("auth.json");
        fs::write(
            &auth,
            r#"{"anthropic":{"type":"oauth","access":"x"},"openrouter":{"type":"api_key","key":"x"}}"#,
        )
        .expect("auth");
        let mut accounts = Accounts::read(&AccountFiles {
            pi: Some(auth),
            ..AccountFiles::default()
        });
        let pi = |accounts: &mut Accounts, provider| {
            accounts.resolve(Agent::Pi, Billing::Unknown, Some(provider), None)
        };
        assert_eq!(pi(&mut accounts, "anthropic"), Billing::Subscription);
        assert_eq!(pi(&mut accounts, "openrouter"), Billing::Metered);
        assert_eq!(pi(&mut accounts, "github-copilot"), Billing::Subscription);
        assert_eq!(pi(&mut accounts, "openai"), Billing::Metered);
        assert_eq!(
            accounts.resolve(Agent::Flsh, Billing::Unknown, None, None),
            Billing::Unknown
        );
        assert_eq!(
            flsh_provider("openrouter:example/model"),
            Some("openrouter")
        );
    }

    #[test]
    fn bedrock_and_vertex_requests_are_metered() {
        assert!(claude_cloud(Some("msg_bdrk_01"), "claude-opus-5-5"));
        assert!(claude_cloud(None, "us.anthropic.claude-opus-5-5-v1:0"));
        assert!(claude_cloud(None, "claude-opus-5-5@20261101"));
        assert!(!claude_cloud(Some("msg_01"), "claude-opus-5-5"));
    }
}
