//! Spots a model stuck emitting the same text, or making the same tool call,
//! over and over.

use async_trait::async_trait;
use flashmind::types::{
    ToolRegistry,
    tool::{Tool, ToolContext, ToolResult},
};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};

/// How many back-to-back copies of a chunk count as a loop.
const COPIES: usize = 4;
/// Shorter chunks are not checked on their own; a short chunk repeated long
/// enough still shows up as a longer one.
const MIN_PERIOD: usize = 24;
const MAX_PERIOD: usize = 600;
/// Only re-check after this many new characters, so the scan stays cheap.
const CHECK_EVERY: usize = 128;

#[derive(Default)]
pub struct RepeatGuard {
    tail: Vec<char>,
    unchecked: usize,
}

impl RepeatGuard {
    /// Adds streamed text. Returns true once the tail of everything seen is
    /// one chunk repeated [`COPIES`] times.
    pub fn push(&mut self, delta: &str) -> bool {
        let before = self.tail.len();
        self.tail.extend(delta.chars());
        self.unchecked += self.tail.len() - before;
        let keep = MAX_PERIOD * COPIES;
        if self.tail.len() > keep {
            self.tail.drain(..self.tail.len() - keep);
        }
        if self.unchecked < CHECK_EVERY {
            return false;
        }
        self.unchecked = 0;
        (MIN_PERIOD..=MAX_PERIOD)
            .take_while(|period| period * COPIES <= self.tail.len())
            .any(|period| self.repeats(period))
    }

    fn repeats(&self, period: usize) -> bool {
        let tail = &self.tail[self.tail.len() - period * COPIES..];
        let (first, rest) = tail.split_at(period);
        // Whitespace alone repeating is not a loop worth stopping for.
        !first.iter().all(|c| c.is_whitespace()) && rest.chunks(period).all(|chunk| chunk == first)
    }
}

/// How many times one run may call a tool with the same arguments.
const SAME_CALLS: usize = 2;

/// Tools that read state that changes between calls, so calling them again
/// with the same arguments is not a loop.
const LIVE_TOOLS: &[&str] = &[
    "browser",
    "read_screen",
    "get_notifications",
    "get_location",
    "get_time",
    "task_progress",
];

/// Wraps every tool in `registry` so a run that calls one with the same
/// arguments more than [`SAME_CALLS`] times gets an error back instead of
/// another identical result. Call it once per run, after the last tool is
/// registered.
pub fn guard_tool_calls(registry: &mut ToolRegistry) {
    let calls = Arc::new(Mutex::new(HashMap::new()));
    let tools: Vec<Arc<dyn Tool>> = registry
        .list()
        .into_iter()
        .filter(|name| !LIVE_TOOLS.contains(name))
        .filter_map(|name| registry.get(name).cloned())
        .collect();
    for tool in tools {
        registry.register(Arc::new(CallGuard {
            tool,
            calls: calls.clone(),
        }));
    }
}

struct CallGuard {
    tool: Arc<dyn Tool>,
    /// Calls so far in this run, by tool name and arguments.
    calls: Arc<Mutex<HashMap<String, usize>>>,
}

#[async_trait]
impl Tool for CallGuard {
    fn name(&self) -> &str {
        self.tool.name()
    }

    fn description(&self) -> &str {
        self.tool.description()
    }

    fn parameters(&self) -> Value {
        self.tool.parameters()
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let key = format!("{}\0{}", self.tool.name(), ctx.args);
        let count = {
            let mut calls = self.calls.lock().unwrap_or_else(PoisonError::into_inner);
            let count = calls.entry(key).or_default();
            *count += 1;
            *count
        };
        if count > SAME_CALLS {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "You already made this exact call and it will return the same result. \
                 Change the arguments, try another tool, or answer with what you have.",
            ));
        }
        self.tool.execute(ctx).await
    }

    fn timeout_secs(&self) -> Option<u64> {
        self.tool.timeout_secs()
    }

    fn max_output_bytes(&self) -> usize {
        self.tool.max_output_bytes()
    }

    fn max_output_lines(&self) -> usize {
        self.tool.max_output_lines()
    }

    fn humanize(&self, args: &Value) -> String {
        self.tool.humanize(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flashmind::core::CancellationToken;
    use serde_json::json;

    struct Echo;

    #[async_trait]
    impl Tool for Echo {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "Echoes"
        }

        fn parameters(&self) -> Value {
            json!({"type": "object"})
        }

        async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
            Ok(ToolResult::success(ctx.tool_call_id, ctx.args.to_string()))
        }
    }

    #[tokio::test]
    async fn refuses_the_same_tool_call_after_two_tries() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(Echo));
        guard_tool_calls(&mut registry);
        let tool = registry.get("echo").expect("echo registered");
        let cancel = CancellationToken::new();
        let call = |args: Value| tool.execute(ToolContext::new("call", args, None, &cancel));

        assert!(call(json!({"q": "a"})).await.unwrap().is_success());
        assert!(call(json!({"q": "a"})).await.unwrap().is_success());
        assert!(!call(json!({"q": "a"})).await.unwrap().is_success());
        assert!(call(json!({"q": "b"})).await.unwrap().is_success());
    }

    #[test]
    fn stops_a_repeated_sentence() {
        let mut guard = RepeatGuard::default();
        let looped = (0..20).any(|_| guard.push("I will check the inbox now. "));
        assert!(looped);
    }

    #[test]
    fn leaves_normal_text_alone() {
        let mut guard = RepeatGuard::default();
        for i in 0..200 {
            assert!(!guard.push(&format!("Line {i} says something different. ")));
        }
        let mut guard = RepeatGuard::default();
        assert!(!guard.push("| a | b |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |\n"));
        let mut guard = RepeatGuard::default();
        assert!(!guard.push(&" ".repeat(5000)));
    }
}
