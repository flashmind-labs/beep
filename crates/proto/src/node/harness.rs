//! The coding agent harnesses on a node (Claude Code, Codex, pi, ...): what
//! the node finds of each, asked with `SessionsRequest::Harnesses` on
//! `agents/1`, and the user's settings for them, which the server sends
//! with `Down::AgentSettings` and the node applies when it starts one.
//!
//! Like the rest of the node link these are compact msgpack, so a field is
//! added only at the end with `#[serde(default)]`.

mod output;

pub use output::{clean_output, failure_line};

use super::EnvSecret;
use serde::{Deserialize, Serialize};

/// A harness Beep knows: where it keeps its home and how it installs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Harness {
    /// Its slug, the agent's name on the node, such as `claude`.
    pub agent: &'static str,
    /// The variable that moves its home and config folder.
    pub home_var: &'static str,
    /// Its home folder when that variable is not set.
    pub home_default: &'static str,
    /// The npm package of its CLI, when it ships on npm.
    pub npm: Option<&'static str>,
}

/// The harnesses Beep knows, in the order the apps list them.
pub const HARNESSES: &[Harness] = &[
    Harness {
        agent: "claude",
        home_var: "CLAUDE_CONFIG_DIR",
        home_default: "~/.claude",
        npm: Some("@anthropic-ai/claude-code"),
    },
    Harness {
        agent: "codex",
        home_var: "CODEX_HOME",
        home_default: "~/.codex",
        npm: Some("@openai/codex"),
    },
    Harness {
        agent: "pi",
        home_var: "PI_CODING_AGENT_DIR",
        home_default: "~/.pi/agent",
        npm: Some("@mariozechner/pi-coding-agent"),
    },
    Harness {
        agent: "gemini",
        home_var: "GEMINI_CLI_HOME",
        home_default: "~",
        npm: Some("@google/gemini-cli"),
    },
    Harness {
        agent: "flsh",
        home_var: "FLASHMIND_HOME",
        home_default: "~/.flashmind",
        npm: None,
    },
    Harness {
        agent: "opencode",
        home_var: "OPENCODE_CONFIG_DIR",
        home_default: "~/.config/opencode",
        npm: Some("opencode-ai"),
    },
];

/// The harness `agent` names, when Beep knows it.
pub fn harness(agent: &str) -> Option<&'static Harness> {
    HARNESSES.iter().find(|harness| harness.agent == agent)
}

/// What a node found of each harness, for `SessionsReply::Harnesses`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HarnessReport {
    pub harnesses: Vec<HarnessInfo>,
    /// When the newest versions were last looked up, in Unix seconds; 0
    /// when never.
    pub checked_at: u64,
}

/// One harness on a node.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HarnessInfo {
    pub agent: String,
    /// Its CLI is on the node's `PATH`.
    pub installed: bool,
    /// The node can start it: the CLI and the ACP adapter it needs are
    /// there. Only these are offered.
    pub runnable: bool,
    /// Its version, such as `2.1.312`, as its `--version` says.
    pub version: Option<String>,
    /// The newest version published, when it could be looked up.
    pub latest: Option<String>,
    /// Where the CLI is.
    pub cli_path: Option<String>,
    /// What the node starts for a session, with the user's settings left
    /// out: the CLI itself or its ACP adapter.
    pub program: Option<String>,
    /// Whether it is signed in, when that can be read on the machine.
    pub signed_in: Option<bool>,
    /// Who it is signed in as, when it says.
    pub account: Option<String>,
    /// The plan of that account, such as `max`, when it says.
    pub plan: Option<String>,
    /// The command `UpdateHarness` runs: an install while it is missing,
    /// an update after. `None` when the node knows of none.
    pub install: Option<String>,
}

/// The user's settings for the harnesses on one machine, sent in full
/// after the welcome and whenever they change. A harness with none runs
/// as the node finds it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    pub harnesses: Vec<HarnessConfig>,
    /// The user's flsh settings, the same on every machine, which the node
    /// writes to flsh's config before it starts flsh. `None` until the
    /// user saves some, and from a server before them: the node then
    /// leaves flsh's config alone.
    pub flsh: Option<crate::FlshSettings>,
}

/// How the node starts one harness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessConfig {
    pub agent: String,
    /// Off: the node refuses to start it and the server does not offer it.
    #[serde(default = "on")]
    pub enabled: bool,
    /// The program to start in place of the one the node found.
    #[serde(default)]
    pub program: Option<String>,
    /// The folder its home variable (`Harness::home_var`) is set to.
    #[serde(default)]
    pub home: Option<String>,
    /// Added after the node's own arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Set on its process, over everything else.
    #[serde(default)]
    pub env: Vec<EnvSecret>,
    /// When the node compacts its sessions. `None` is the default: on, at
    /// `AutoCompact::DEFAULT_PERCENT`.
    #[serde(default)]
    pub compact: Option<AutoCompact>,
}

/// Whether and when the node compacts a harness's sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoCompact {
    /// Off: the node never sends `/compact`.
    pub enabled: bool,
    /// Compact once the conversation fills this percent of the window, 1 to
    /// 100. `None` is `DEFAULT_PERCENT`.
    #[serde(default)]
    pub percent: Option<u8>,
}

impl AutoCompact {
    /// The percent used when none is set.
    pub const DEFAULT_PERCENT: u8 = 80;

    /// Whether `percent` is one the node accepts.
    pub fn valid_percent(percent: u8) -> bool {
        (1..=100).contains(&percent)
    }
}

fn on() -> bool {
    true
}

impl HarnessConfig {
    /// The settings of a harness nobody changed.
    pub fn new(agent: &str) -> Self {
        Self {
            agent: agent.to_owned(),
            enabled: true,
            program: None,
            home: None,
            args: Vec::new(),
            env: Vec::new(),
            compact: None,
        }
    }
}

/// The version in what a CLI's `--version` printed, such as `2.1.312` in
/// `2.1.312 (Claude Code)` or `codex-cli 0.166.0`: the first word that
/// starts with a digit (after an optional `v`) and has a dot in it.
pub fn parse_version(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|word| {
        let word = word.trim_matches(|c: char| matches!(c, '(' | ')' | ',' | ';' | '"'));
        let word = word.strip_prefix('v').unwrap_or(word);
        let starts = word.chars().next().is_some_and(|c| c.is_ascii_digit());
        let clean = word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
        (starts && clean && word.contains('.')).then(|| word.to_owned())
    })
}

/// Whether `latest` is newer than `current`, comparing the dotted numbers
/// of each; a pre-release (`1.2.0-beta`) counts as its numbers.
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn numbers(version: &str) -> Vec<u64> {
        version
            .split(['-', '+'])
            .next()
            .unwrap_or_default()
            .split('.')
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    }
    let (latest, current) = (numbers(latest), numbers(current));
    let len = latest.len().max(current.len());
    let at = |list: &[u64], i: usize| list.get(i).copied().unwrap_or(0);
    for i in 0..len {
        match at(&latest, i).cmp(&at(&current, i)) {
            std::cmp::Ordering::Greater => return true,
            std::cmp::Ordering::Less => return false,
            std::cmp::Ordering::Equal => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SecretText;

    #[test]
    fn versions_are_read_from_what_each_cli_prints() {
        assert_eq!(
            parse_version("2.1.312 (Claude Code)").as_deref(),
            Some("2.1.312")
        );
        assert_eq!(
            parse_version("codex-cli 0.166.0").as_deref(),
            Some("0.166.0")
        );
        assert_eq!(parse_version("v0.99.2\n").as_deref(), Some("0.99.2"));
        assert_eq!(
            parse_version("flsh 0.41.0-beta.1").as_deref(),
            Some("0.41.0-beta.1")
        );
        assert_eq!(parse_version("opencode"), None);
        assert_eq!(parse_version("error: 42 things"), None);
    }

    #[test]
    fn newer_compares_numbers_not_text() {
        assert!(is_newer("2.1.320", "2.1.312"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.99.2"));
        assert!(!is_newer("2.1.312", "2.1.312"));
        assert!(!is_newer("2.1.3", "2.1.312"));
        assert!(!is_newer("1.2.0-beta", "1.2.0"));
    }

    #[test]
    fn settings_round_trip_hide_their_values_and_default_to_on() {
        let settings = AgentSettings {
            harnesses: vec![HarnessConfig {
                env: vec![EnvSecret {
                    name: "HTTPS_PROXY".into(),
                    value: SecretText::new("http://user:pass7777@proxy"),
                }],
                args: vec!["--verbose".into()],
                ..HarnessConfig::new("codex")
            }],
            flsh: Some(crate::FlshSettings {
                zdr: Some(true),
                quantizations: vec![crate::FlshQuantization::Fp8],
                sort: Some(crate::FlshSort::Throughput),
                models: vec![crate::FlshModelRule {
                    model: "openrouter:vendor/model".parse().unwrap(),
                    autocompact: crate::FlshAutoCompact {
                        enabled: true,
                        tokens: Some(120_000),
                    },
                }],
                ..crate::FlshSettings::default()
            }),
        };
        let body = rmp_serde::to_vec_named(&settings).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<AgentSettings>(&body).unwrap(),
            settings
        );
        assert!(!format!("{settings:?}").contains("7777"));
        #[derive(Serialize)]
        struct Older {
            agent: &'static str,
        }
        let body = rmp_serde::to_vec_named(&Older { agent: "pi" }).unwrap();
        let read: HarnessConfig = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(read, HarnessConfig::new("pi"));
    }
}
