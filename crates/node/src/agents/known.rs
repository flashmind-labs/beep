//! The agents the node knows how to start (`KNOWN`) and how each one's ACP adapter runs.

use super::compaction::Route;

pub(super) const CODEX_CONFIG: &str = "CODEX_CONFIG";
/// OpenCode's permission rules, as JSON.
pub(super) const OPENCODE_PERMISSION: &str = "OPENCODE_PERMISSION";

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
    /// launch cannot set it, set once the session exists.
    pub(super) bypass_mode: Option<&'static str>,
    /// The id of the session mode that asks no permission, however the
    /// launch reaches it: shown first as Full access, and a child is never
    /// started above its parent's.
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

/// The ACP adapters of the known agents, which the WSL probe looks for.
pub(crate) fn adapter_bins() -> impl Iterator<Item = &'static str> {
    KNOWN.iter().filter_map(Known::adapter_bin)
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
    // Google's Antigravity CLI. The flag approves every tool in `default`
    // mode; `plan` and `accept-edits` are its other ACP session modes.
    Known {
        name: "agy",
        title: "Antigravity",
        cli: "agy",
        adapter: Adapter::Flags(&["acp", "--dangerously-skip-permissions"]),
        env: &[],
        thinking_env: &[],
        bypass_mode: None,
        full_mode: Some("default"),
        read_only_mode: None,
        model_flag: false,
        compact: Route::Threshold {
            var: "AGY_CLI_AUTO_COMPACT_OVERRIDE",
        },
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
        env: &[(OPENCODE_PERMISSION, r#"{"*":"allow"}"#)],
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
