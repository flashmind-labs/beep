//! The agents a node knows how to start, and how it starts them.

use super::{Route, adapters, locate};
use beep_proto::ModelId;
use beep_proto::node::harness::{AutoCompact, ModelCompact};
use serde::{Deserialize, Serialize};
use crate::machine::wsl::{DistroName, LinuxPath};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(super) const CODEX_CONFIG: &str = "CODEX_CONFIG";

/// An agent this node can start: a program that speaks ACP on its stdio.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentKind {
    /// What Beep calls it, such as `claude`; its protocol is `claude/1`.
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Variables set for it on top of the node's own environment.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// The user's auto-compact setting; `None` is the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact: Option<AutoCompact>,
    #[serde(default)]
    pub compact_models: Vec<ModelCompact>,
    #[serde(default)]
    pub default_model: Option<ModelId>,
    /// The node's bridge it runs on, when it does not speak ACP itself.
    #[serde(skip)]
    pub bridge: Option<Bridge>,
}

/// Shows the names of its variables, never their values, which may be the
/// user's secrets.
impl std::fmt::Debug for AgentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentKind")
            .field("name", &self.name)
            .field("program", &self.program)
            .field("args", &self.args)
            .field("env", &self.env.keys().collect::<Vec<_>>())
            .field("bridge", &self.bridge)
            .finish()
    }
}

/// An agent the daemon knows how to start, when its CLI is installed.
/// Each starts with its own permission prompts off, so it never stops to
/// ask; a permission request that comes anyway is allowed by the session.
pub(super) struct Known {
    pub(super) name: &'static str,
    /// What people call it, such as Claude Code.
    pub(super) title: &'static str,
    /// The CLI whose presence says the user has this agent.
    pub(super) cli: &'static str,
    pub(super) adapter: Adapter,
    /// Variables that turn its permission prompts off.
    pub(super) env: &'static [(&'static str, &'static str)],
    /// Variables that make it show its thinking, set unless the node's own
    /// environment sets them.
    pub(super) thinking_env: &'static [(&'static str, &'static str)],
    /// The session mode that asks no permission, for an agent whose
    /// launch cannot set it, set once the session exists. Switching modes
    /// shows it as Full access, first.
    pub(super) bypass_mode: Option<&'static str>,
    /// The id of the session mode that asks no permission, however the
    /// launch reaches it: a child is never started above its parent's.
    pub(super) full_mode: Option<&'static str>,
    /// The id of the session mode that only reads, which a reviewer starts
    /// in; `None` when the agent offers none.
    pub(super) read_only_mode: Option<&'static str>,
    /// It starts on a model given as `--model` on its command line; the
    /// others switch over ACP once the session exists.
    pub(super) model_flag: bool,
    /// How it is asked to compact.
    pub(super) compact: Route,
    /// It reports the commands it runs in the background when the client
    /// says it takes them (Claude Code's `asyncTasks`), and stops one on
    /// request. Saying so also makes its adapter treat the client as AIR.
    pub(super) async_tasks: bool,
}

/// How the node runs an agent it does not start as is: on one of its own
/// bridges, a task inside the node that speaks ACP to the session and
/// drives the agent's CLI, `program` with `args` after the bridge's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bridge {
    /// Claude Code's CLI in stream-json mode, inside the WSL distribution
    /// `wsl` when given, whose user's home is `home`.
    Claude {
        wsl: Option<DistroName>,
        home: Option<LinuxPath>,
    },
    /// Codex's `app-server`, inside the WSL distribution `wsl` when given.
    Codex { wsl: Option<DistroName> },
    /// An ACP agent in the WSL distribution `wsl`, its paths turned across.
    Relay { wsl: DistroName },
}

impl Bridge {
    /// The WSL distribution the agent runs in.
    pub fn wsl(&self) -> Option<&DistroName> {
        match self {
            Self::Claude { wsl, .. } | Self::Codex { wsl } => wsl.as_ref(),
            Self::Relay { wsl } => Some(wsl),
        }
    }
}

/// `kind` runs inside WSL, which only this node can start: its program and
/// arguments stay as detection made them.
pub(super) fn in_wsl(kind: &AgentKind) -> bool {
    kind.bridge.as_ref().is_some_and(|bridge| bridge.wsl().is_some())
}

/// The ACP adapters of the known agents, which the WSL probe looks for.
pub(crate) fn adapter_bins() -> impl Iterator<Item = &'static str> {
    KNOWN.iter().filter_map(Known::adapter_bin)
}

/// `kind`, Claude Code or Codex, run by the node's own bridge on the CLI
/// found here. `None` for another agent. An agent in WSL stays as it was
/// found: Claude Code on the bridge, Codex on its relay.
pub(super) fn direct(kind: &AgentKind) -> Option<AgentKind> {
    let bridge = match kind.name.as_str() {
        _ if kind.bridge.is_some() => return Some(kind.clone()),
        "claude" => Bridge::Claude { wsl: None, home: None },
        "codex" => Bridge::Codex { wsl: None },
        _ => return None,
    };
    let program = locate::find(&kind.name, dirs::home_dir().as_deref())
        .map_or_else(|| kind.name.clone(), |path| path.display().to_string());

    Some(AgentKind {
        program,
        args: Vec::new(),
        bridge: Some(bridge),
        ..kind.clone()
    })
}

/// How the agent `name` is asked to compact: one the node does not know
/// takes `/compact` when it advertises it.
pub(super) fn route(name: &str) -> Route {
    known(name).map_or(Route::Advertised, |known| known.compact)
}

/// What the node knows of the agent `name`, when it is a known one.
pub(super) fn known(name: &str) -> Option<&'static Known> {
    KNOWN.iter().find(|known| known.name == name)
}

impl Known {
    /// The program of its npm ACP adapter, when it has one.
    pub(super) fn adapter_bin(&self) -> Option<&'static str> {
        match self.adapter {
            Adapter::Npm { bin, .. } => Some(bin),
            Adapter::Flags(_) => None,
        }
    }
}

pub(super) enum Adapter {
    /// An ACP adapter from npm: its binary when installed, the one the node
    /// installed in its folder, or run with npx.
    Npm {
        bin: &'static str,
        package: &'static str,
        /// The oldest version the node runs: an older one it installed is
        /// installed again, and npx fetches one at least this new.
        min: &'static str,
    },
    /// The CLI itself speaks ACP with these arguments.
    Flags(&'static [&'static str]),
}

pub(super) const KNOWN: &[Known] = &[
    // The session switches it to `bypassPermissions` once it has one.
    Known {
        name: "claude",
        title: "Claude Code",
        cli: "claude",
        adapter: Adapter::Npm {
            bin: "claude-agent-acp",
            package: "@agentclientprotocol/claude-agent-acp",
            min: "0.101.0",
        },
        // Claude Code leaves its task tools off on recent models; its
        // adapter reports their calls as the session's plan.
        env: &[("CLAUDE_CODE_ENABLE_TODO_TOOLS", "1")],
        // Its adapter streams thinking only as summaries: recent models
        // default to `omitted`, which sends empty thinking blocks.
        thinking_env: &[("CLAUDE_CODE_THINKING_DISPLAY", "summarized")],
        bypass_mode: Some("bypassPermissions"),
        full_mode: Some("bypassPermissions"),
        read_only_mode: Some("plan"),
        model_flag: false,
        compact: Route::Percent {
            percent_var: "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",
            off_var: "DISABLE_AUTO_COMPACT",
        },
        async_tasks: true,
    },
    // Full access: approval policy `never` and sandbox `danger-full-access`,
    // for new and resumed sessions alike.
    Known {
        name: "codex",
        title: "Codex",
        cli: "codex",
        adapter: Adapter::Npm {
            bin: "codex-acp",
            package: "@agentclientprotocol/codex-acp",
            // 2.7.0 forwards Codex's questions asked mid-turn.
            min: "2.7.0",
        },
        env: &[("INITIAL_AGENT_MODE", "agent-full-access")],
        thinking_env: &[],
        bypass_mode: None,
        full_mode: Some("agent-full-access"),
        read_only_mode: Some("read-only"),
        model_flag: false,
        compact: Route::TokenLimit {
            config_var: CODEX_CONFIG,
            key: "model_auto_compact_token_limit",
        },
        async_tasks: false,
    },
    // YOLO approves every tool. Gemini CLI drops back to `default` in a
    // folder it does not trust, so the folder is trusted too.
    Known {
        name: "gemini",
        title: "Gemini CLI",
        cli: "gemini",
        adapter: Adapter::Flags(&["--acp", "--approval-mode", "yolo"]),
        env: &[("GEMINI_CLI_TRUST_WORKSPACE", "true")],
        thinking_env: &[],
        bypass_mode: None,
        full_mode: Some("yolo"),
        read_only_mode: None,
        model_flag: true,
        compact: Route::OwnTrigger,
        async_tasks: false,
    },
    // Google's Antigravity CLI, which replaces Gemini CLI. It has no
    // session modes; the flag approves every tool. Model and thinking are
    // its ACP config options.
    Known {
        name: "agy",
        title: "Antigravity",
        cli: "agy",
        adapter: Adapter::Flags(&["acp", "--dangerously-skip-permissions"]),
        env: &[],
        thinking_env: &[],
        bypass_mode: None,
        full_mode: None,
        read_only_mode: None,
        model_flag: false,
        compact: Route::OwnTrigger,
        async_tasks: false,
    },
    // pi has no permission prompts; only its extensions ask.
    Known {
        name: "pi",
        title: "pi",
        cli: "pi",
        adapter: Adapter::Npm {
            bin: "pi-acp",
            package: "pi-acp",
            min: "0.0.34",
        },
        env: &[],
        thinking_env: &[],
        bypass_mode: None,
        full_mode: None,
        read_only_mode: None,
        model_flag: false,
        compact: Route::Advertised,
        async_tasks: false,
    },
    // flsh never asks permission.
    Known {
        name: "flsh",
        title: "flsh",
        cli: "flsh",
        adapter: Adapter::Flags(&["acp"]),
        env: &[],
        thinking_env: &[],
        bypass_mode: None,
        full_mode: None,
        read_only_mode: None,
        model_flag: true,
        compact: Route::OwnTrigger,
        async_tasks: false,
    },
    // Hermes speaks ACP itself and keeps its provider setup and memory.
    Known {
        name: "hermes",
        title: "Hermes",
        cli: "hermes",
        adapter: Adapter::Flags(&["acp"]),
        env: &[],
        thinking_env: &[],
        bypass_mode: Some("dont_ask"),
        full_mode: Some("dont_ask"),
        read_only_mode: None,
        model_flag: false,
        compact: Route::OwnTrigger,
        async_tasks: false,
    },
    // Merged over its config as the last rule, which wins over the defaults
    // that ask (`doom_loop`, `external_directory`, reading `.env` files).
    Known {
        name: "opencode",
        title: "OpenCode",
        cli: "opencode",
        adapter: Adapter::Flags(&["acp"]),
        env: &[("OPENCODE_PERMISSION", r#"{"*":"allow"}"#)],
        thinking_env: &[],
        bypass_mode: None,
        full_mode: None,
        read_only_mode: None,
        model_flag: false,
        compact: Route::Toggle {
            off_var: "OPENCODE_DISABLE_AUTOCOMPACT",
        },
        async_tasks: false,
    },
];

/// The agents installed here, from `KNOWN`, then `extra` from `node.toml`
/// (which replace a known one of the same name). A program not on `PATH` is
/// looked for in the usual install folders under home, and started from
/// where it was found. An ACP adapter the node installed in its folder
/// `node_dir` is used when the machine has none of its own.
pub fn detect(extra: &[AgentKind], node_dir: Option<&Path>) -> Vec<AgentKind> {
    let detection = detection(extra, node_dir);
    detection.log();
    detection.kinds
}

/// What detection found.
pub(super) struct Detection {
    /// The agents the node can start.
    pub(super) kinds: Vec<AgentKind>,
    /// The known agents whose CLI is here but lack a direct ACP adapter.
    pub(super) lacking: Vec<&'static Known>,
    /// The agents in `kinds` that run on the adapter the node installed.
    pub(super) on_own_node: Vec<&'static str>,
}

impl Detection {
    fn record(&mut self, known: &'static Known, launch: Option<Launch>, home: Option<&Path>) {
        match launch {
            Some(Launch::Ready(kind)) => self.kinds.push(anchored(kind, home)),
            Some(Launch::ViaNpx(kind)) => {
                self.lacking.push(known);
                self.kinds.push(anchored(kind, home));
            }
            Some(Launch::OnOwnNode(kind)) => {
                self.on_own_node.push(known.name);
                self.kinds.push(beside_cli(kind, known, home));
            }
            Some(Launch::Direct(kind)) => {
                self.lacking.push(known);
                self.kinds.push(kind);
            }
            Some(Launch::LacksAdapter) => self.lacking.push(known),
            None => {}
        }
    }

    pub(super) fn log(&self) {
        for known in &self.lacking {
            log::info!(
                "{} found, but not its ACP adapter ({})",
                known.cli,
                known.adapter_bin().unwrap_or(known.cli)
            );
        }

        let names: Vec<&str> = self.kinds.iter().map(|kind| kind.name.as_str()).collect();
        if !names.is_empty() {
            log::info!("coding agents found: {}", names.join(", "));
        } else if self.lacking.is_empty() {
            let missing: Vec<&str> = KNOWN.iter().map(|known| known.cli).collect();
            log::warn!(
                "no coding agent found ({} not on PATH or in the usual install folders)",
                missing.join(", ")
            );
        }
    }
}

/// `detect` without the logging.
pub(super) fn detection(extra: &[AgentKind], node_dir: Option<&Path>) -> Detection {
    let home = dirs::home_dir();
    let found = |program: &str| locate::find(program, home.as_deref()).is_some();
    let mut detection = Detection {
        kinds: Vec::new(),
        lacking: Vec::new(),
        on_own_node: Vec::new(),
    };

    for known in KNOWN {
        let own = node_dir.and_then(|dir| adapters::installed(dir, known));
        let launch = launch(known, &found, own.as_ref());
        // An outdated adapter of the node's own runs until its new one is in.
        let outdated = own.as_ref().is_some_and(|own| !own.current)
            && matches!(launch, Some(Launch::OnOwnNode(_)));
        detection.record(known, launch, home.as_deref());

        if outdated {
            detection.lacking.push(known);
        }
    }

    let wsl: Vec<AgentKind> = KNOWN
        .iter()
        .filter(|known| !detection.kinds.iter().any(|kind| kind.name == known.name))
        .filter_map(in_wsl_kind)
        .collect();
    detection.kinds.extend(wsl);

    for kind in extra {
        detection.kinds.retain(|known| known.name != kind.name);
        detection.lacking.retain(|known| known.name != kind.name);
        detection.on_own_node.retain(|name| *name != kind.name);
        detection.kinds.push(kind.clone());
    }

    detection
}

/// `known`, installed in a WSL distribution of this Windows machine, which
/// runs it when Windows has none of its own, on the Windows checkout:
/// Claude Code on the node's bridge, any other agent through its relay,
/// with the adapter or `npx` the distribution has.
fn in_wsl_kind(known: &'static Known) -> Option<AgentKind> {
    let (distro, found) = crate::machine::wsl::with_agent(known.name)?;
    let path = |name: &str| {
        found
            .agents
            .iter()
            .chain(&found.tools)
            .find(|program| program.name == name)
            .map(|program| program.path.to_string())
    };

    let (program, args, env, bridge) = if known.name == "claude" {
        let bridge = Bridge::Claude {
            wsl: Some(distro.clone()),
            home: found.home.clone(),
        };
        (path(known.cli)?, Vec::new(), env(known), bridge)
    } else {
        let kind = match launch(known, &|name| path(name).is_some(), None)? {
            Launch::Ready(kind) | Launch::ViaNpx(kind) => kind,
            _ => return None,
        };
        let bridge = Bridge::Relay { wsl: distro.clone() };
        (path(&kind.program)?, kind.args, kind.env, bridge)
    };
    log::info!("{} found in WSL {distro}", known.title);

    Some(AgentKind {
        name: known.name.to_owned(),
        program,
        args,
        env,
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: Some(bridge),
    })
}

/// The variables `known` starts with: its own, and those that show its
/// thinking unless the node's environment sets them.
fn env(known: &Known) -> BTreeMap<String, String> {
    let thinking = known
        .thinking_env
        .iter()
        .filter(|(name, _)| std::env::var_os(name).is_none());
    known
        .env
        .iter()
        .chain(thinking)
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

/// `kind` set to start the program where `locate` found it when `PATH` does
/// not have it, with that folder in front of the agent's own `PATH` so what
/// it starts (`node`, the agent's CLI) is found beside it.
fn anchored(mut kind: AgentKind, home: Option<&Path>) -> AgentKind {
    if which::which(&kind.program).is_ok() {
        return kind;
    }

    let Some(path) = locate::find(&kind.program, home) else {
        return kind;
    };
    log::info!("{} found at {}", kind.name, path.display());
    if let Some(with) = locate::path_with(&[&path]) {
        kind.env
            .insert("PATH".to_owned(), with.to_string_lossy().into_owned());
    }
    kind.program = path.to_string_lossy().into_owned();
    kind
}

/// `kind`, which runs on the node's own `node`, with that Node and the
/// agent's CLI first on its `PATH`: the adapter starts both.
fn beside_cli(mut kind: AgentKind, known: &Known, home: Option<&Path>) -> AgentKind {
    let node = PathBuf::from(&kind.program);
    let cli = locate::find(known.cli, home);
    let mut programs = vec![node.as_path()];
    programs.extend(cli.as_deref());
    if let Some(with) = locate::path_with(&programs) {
        kind.env
            .insert("PATH".to_owned(), with.to_string_lossy().into_owned());
    }
    kind
}

/// How the node can start a known agent whose CLI is installed.
#[derive(Debug, PartialEq)]
pub(super) enum Launch {
    /// With what the machine has: the CLI or its adapter.
    Ready(AgentKind),
    /// Usable while the node installs a direct adapter.
    ViaNpx(AgentKind),
    /// With the adapter the node installed, on the node's own Node.
    OnOwnNode(AgentKind),
    /// Claude Code's CLI is here without its adapter or npx: it runs on the
    /// node's own bridge (`direct`) while the node installs the adapter.
    Direct(AgentKind),
    /// Its CLI is here, but neither its ACP adapter nor npx to run it with:
    /// the node installs the adapter in its own folder.
    LacksAdapter,
}

/// How the node starts `known`, when `found` says its CLI is installed:
/// with the adapter the machine has, else the one the node installed
/// (`own`), else with npx. `None` when the CLI is not here.
pub(super) fn launch(
    known: &Known,
    found: &dyn Fn(&str) -> bool,
    own: Option<&adapters::Installed>,
) -> Option<Launch> {
    if !found(known.cli) {
        return None;
    }

    let mut on_own_node = false;
    let mut bridged = false;
    let (program, args) = match (&known.adapter, own) {
        (Adapter::Npm { bin, .. }, _) if found(bin) => ((*bin).to_owned(), Vec::new()),
        (Adapter::Npm { .. }, Some(own)) => {
            on_own_node = true;
            (
                own.node.to_string_lossy().into_owned(),
                vec![own.script.to_string_lossy().into_owned()],
            )
        }
        (Adapter::Npm { package, min, .. }, None) if found("npx") => (
            "npx".to_owned(),
            vec!["-y".to_owned(), format!("{package}@>={min}")],
        ),
        // Claude Code and Codex run on the node's own bridge meanwhile.
        (Adapter::Npm { .. }, None) if !matches!(known.name, "claude" | "codex") => {
            return Some(Launch::LacksAdapter);
        }
        (Adapter::Npm { .. }, None) => {
            bridged = true;
            (known.cli.to_owned(), Vec::new())
        }
        (Adapter::Flags(flags), _) => (
            known.cli.to_owned(),
            flags.iter().map(|flag| (*flag).to_owned()).collect(),
        ),
    };
    let kind = AgentKind {
        name: known.name.to_owned(),
        program,
        args,
        env: env(known),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    };
    Some(if bridged {
        direct(&kind).map_or(Launch::LacksAdapter, Launch::Direct)
    } else if on_own_node {
        Launch::OnOwnNode(kind)
    } else if kind.program == "npx" {
        Launch::ViaNpx(kind)
    } else {
        Launch::Ready(kind)
    })
}

#[cfg(test)]
#[path = "../tests/agents_kinds.rs"]
mod tests;
