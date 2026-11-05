//! The agents a node knows how to start, and how it starts them.

use super::Route;
use beep_proto::node::harness::AutoCompact;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

/// How the agent `name` is asked to compact: one the node does not know
/// takes `/compact` when it advertises it.
pub(super) fn route(name: &str) -> Route {
    known(name).map_or(Route::Advertised, |known| known.compact)
}

/// What the node knows of the agent `name`, when it is a known one.
pub(super) fn known(name: &str) -> Option<&'static Known> {
    KNOWN.iter().find(|known| known.name == name)
}

pub(super) enum Adapter {
    /// An ACP adapter from npm: its binary when installed, or run with npx.
    Npm {
        bin: &'static str,
        package: &'static str,
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
        },
        env: &[],
        // Its adapter streams thinking only as summaries: recent models
        // default to `omitted`, which sends empty thinking blocks.
        thinking_env: &[("CLAUDE_CODE_THINKING_DISPLAY", "summarized")],
        bypass_mode: Some("bypassPermissions"),
        full_mode: Some("bypassPermissions"),
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
        },
        env: &[("INITIAL_AGENT_MODE", "agent-full-access")],
        thinking_env: &[],
        bypass_mode: None,
        full_mode: Some("agent-full-access"),
        model_flag: false,
        compact: Route::TokenLimit {
            config_var: "CODEX_CONFIG",
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
        model_flag: true,
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
        },
        env: &[],
        thinking_env: &[],
        bypass_mode: None,
        full_mode: None,
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
        model_flag: true,
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
        model_flag: false,
        compact: Route::Toggle {
            off_var: "OPENCODE_DISABLE_AUTOCOMPACT",
        },
        async_tasks: false,
    },
];

/// The agents installed here, from `KNOWN`, then `extra` from `node.toml`
/// (which replace a known one of the same name).
pub fn detect(extra: &[AgentKind]) -> Vec<AgentKind> {
    let found = |program: &str| which::which(program).is_ok();
    let mut kinds: Vec<AgentKind> = KNOWN
        .iter()
        .filter_map(|known| launch(known, &found))
        .collect();
    for kind in extra {
        kinds.retain(|known| known.name != kind.name);
        kinds.push(kind.clone());
    }
    kinds
}

/// How the node starts `known`, when `found` says its CLI, and the adapter
/// or npx it needs, are installed.
pub(super) fn launch(known: &Known, found: &dyn Fn(&str) -> bool) -> Option<AgentKind> {
    if !found(known.cli) {
        return None;
    }
    let (program, args) = match &known.adapter {
        Adapter::Npm { bin, .. } if found(bin) => ((*bin).to_owned(), Vec::new()),
        Adapter::Npm { package, .. } if found("npx") => (
            "npx".to_owned(),
            vec!["-y".to_owned(), (*package).to_owned()],
        ),
        Adapter::Npm { .. } => return None,
        Adapter::Flags(flags) => (
            known.cli.to_owned(),
            flags.iter().map(|flag| (*flag).to_owned()).collect(),
        ),
    };
    let thinking = known
        .thinking_env
        .iter()
        .filter(|(name, _)| std::env::var_os(name).is_none());
    let env = known
        .env
        .iter()
        .chain(thinking)
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    Some(AgentKind {
        name: known.name.to_owned(),
        program,
        args,
        env,
        compact: None,
    })
}
