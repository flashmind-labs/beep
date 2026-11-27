//! One process of the machine's table, and which are an agent's own helpers.

use std::sync::Arc;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Process {
    pub(crate) pid: u32,
    pub(crate) parent: u32,
    pub(crate) group: u32,
    /// The OS creation time, kept at its original precision to detect pid reuse.
    pub(crate) birth: u64,
    pub(crate) started_at: u64,
    pub(crate) command: Arc<str>,
}

/// The coding agents' own programs, never a command they ran.
const HARNESSES: [&str; 7] = ["claude", "codex", "agy", "opencode", "pi", "flsh", "hermes"];
/// The npm packages an agent's CLI runs from, as Node starts it.
const HARNESS_PACKAGES: [&str; 2] = ["@openai/codex", "@anthropic-ai/claude-code"];

impl Process {
    pub(crate) fn helper(&self) -> bool {
        let mut words = self.command.split_whitespace();
        let Some(program) = words.next() else {
            return true;
        };
        let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
        let name = name.strip_suffix(".exe").unwrap_or(name);
        // `codex-code-mode-host` and the like are an agent's own helpers.
        let harness = HARNESSES.iter().any(|h| {
            name.strip_prefix(h)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
        });

        if harness
            || matches!(name, "chrome_crashpad_handler" | "bwrap" | "sandbox-exec")
            || name.ends_with("-acp")
        {
            return true;
        }

        let args: Vec<_> = words.collect();
        // Codex keeps its JavaScript and computer-use runtimes alive between calls.
        let program = program.replace('\\', "/");

        if name == "node_repl" && program.ends_with("/cua_node/bin/node_repl")
            || name == "node"
                && args.iter().any(|arg| {
                    arg.replace('\\', "/")
                        .ends_with("/node_modules/@oai/cua-repl/bin/cua-repl.mjs")
                })
        {
            return true;
        }

        if args.windows(2).any(|pair| pair == ["mcp", "--session"]) {
            return true;
        }
        // npm may put a shell or Node between the adapter and its CLI, and
        // Node runs the CLI itself from its package.
        matches!(name, "node" | "sh" | "bash" | "npm" | "npx")
            && args.iter().any(|arg| {
                let arg = arg.replace('\\', "/");
                ["claude-agent-acp", "codex-acp", "pi-acp"]
                    .iter()
                    .any(|adapter| arg.split('/').any(|part| part == *adapter))
                    || HARNESS_PACKAGES
                        .iter()
                        .any(|package| arg.contains(&format!("{package}/")))
            })
    }
}
