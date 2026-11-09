//! Spots a model stuck repeating the same text or tool call.

use crate::agent::paging;
use async_trait::async_trait;
use flashmind::types::{
    ToolRegistry,
    tool::{Tool, ToolContext, ToolResult},
};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
};

/// How many back-to-back copies of a chunk count as a loop.
const COPIES: usize = 4;
/// Shortest chunk checked; shorter repeats still show up as longer ones.
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
    /// Adds streamed text; true once the tail is one chunk repeated [`COPIES`] times.
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

/// How many refused calls end the run.
const MAX_REFUSALS: usize = 3;

/// Tools whose state changes between calls, so repeating them is no loop.
const LIVE_TOOLS: &[&str] = &["get_location", "get_time", "task_progress"];

/// Refuses calls repeated more than [`SAME_CALLS`] times; call once per run, after the last tool is registered.
pub fn guard_tool_calls(registry: &mut ToolRegistry) -> Refusals {
    let calls = Arc::new(Mutex::new(HashMap::new()));
    let refusals = Refusals::default();
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
            refusals: refusals.clone(),
        }));
    }

    refusals
}

/// Counts the calls a run's guard refused.
#[derive(Clone, Default)]
pub struct Refusals(Arc<AtomicUsize>);

impl Refusals {
    /// True once the model has ignored enough refusals.
    pub fn exhausted(&self) -> bool {
        self.0.load(Ordering::Relaxed) >= MAX_REFUSALS
    }
}

struct CallGuard {
    tool: Arc<dyn Tool>,
    /// Calls so far in this run, by tool name and arguments.
    calls: Arc<Mutex<HashMap<String, usize>>>,
    refusals: Refusals,
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
            self.refusals.0.fetch_add(1, Ordering::Relaxed);
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "You already made this exact call and it will return the same result. \
                 Change the arguments, try another tool, or answer with what you have.",
            ));
        }

        let mut result = self.tool.execute(ctx).await?;
        paging::rewrite_text(&mut result, |output| Some(paging::mark(output)));
        Ok(result)
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

    fn concurrent(&self) -> bool {
        self.tool.concurrent()
    }
}

#[cfg(test)]
#[path = "../tests/agent_repeat.rs"]
mod tests;
