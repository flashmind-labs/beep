use super::*;
use crate::{
    agent::subagents::{Decision, Kind},
    research::RoleModel,
};
use async_trait::async_trait;
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, FinishReason, LlmProvider, Provider,
    ReasoningLevel, Role, StreamEvent, TokenUsage,
    tool::{Tool, ToolContext, ToolResult},
};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

const KIND: Kind = Kind {
    name: "test",
    instructions: "You pick a colour.",
    max_turns: 3,
    max_tokens: 1000,
    time_limit: Duration::from_secs(10),
};

/// How the fake model behaves.
#[derive(Clone, Copy)]
enum Script {
    /// Calls `pick` with red.
    Decides,
    /// Calls `pick` with a colour it does not know, then red.
    RetriesBadArguments,
    /// Reports more tokens than the budget.
    Expensive,
    /// Calls a lookup tool forever.
    Loops,
}

struct Fake(Script);

#[async_trait]
impl LlmProvider for Fake {
    fn name(&self) -> &str {
        "test"
    }
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }
    fn complete(&self, request: CompletionRequest) -> CompletionStream {
        let tool_results = request
            .messages
            .iter()
            .filter(|message| message.role == Role::Tool)
            .count();
        match self.0 {
            Script::Decides => call("pick", json!({"colour": "red"}), 10),
            Script::RetriesBadArguments if tool_results == 0 => {
                call("pick", json!({"colour": "plaid"}), 10)
            }
            Script::RetriesBadArguments => call("pick", json!({"colour": "red"}), 10),
            Script::Expensive => call("lookup", json!({"n": tool_results}), 5000),
            Script::Loops => call("lookup", json!({"n": tool_results}), 10),
        }
    }
}

fn usage(tokens: u32) -> StreamEvent {
    StreamEvent::Usage(TokenUsage {
        prompt_tokens: tokens,
        completion_tokens: 0,
        total_tokens: tokens,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
    })
}

fn call(name: &str, arguments: Value, tokens: u32) -> CompletionStream {
    let name = name.to_owned();
    Box::pin(async_stream::stream! {
        yield Ok(StreamEvent::ToolCallStart { index: 0, id: Uuid::new_v4().to_string(), name });
        yield Ok(StreamEvent::ToolCallDelta { index: 0, arguments: arguments.to_string() });
        yield Ok(usage(tokens));
        yield Ok(StreamEvent::Finished(FinishReason::ToolCalls));
    })
}

#[derive(Debug, PartialEq)]
struct Colour(String);

fn pick() -> Decision<Colour> {
    Decision {
        name: "pick",
        description: "Pick a colour.",
        parameters: || json!({"type": "object", "properties": {"colour": {"type": "string"}}}),
        parse: |args| match args["colour"].as_str() {
            Some(colour @ ("red" | "blue")) => Ok(Colour(colour.to_owned())),
            _ => Err("colour must be red or blue".to_owned()),
        },
    }
}

/// A read tool that always answers.
struct Lookup;

#[async_trait]
impl Tool for Lookup {
    fn name(&self) -> &str {
        "lookup"
    }
    fn description(&self) -> &str {
        "Look something up."
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"n": {"type": "integer"}}})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        Ok(ToolResult::success(ctx.tool_call_id, "nothing"))
    }
}

fn subagents(script: Script) -> Subagents {
    let model = "openai:test".parse().expect("a model id");
    Subagents::new(Arc::new(Fake(script)), AgentLlmConfig::new(model))
}

fn spec() -> Spec<Colour> {
    Spec {
        kind: &KIND,
        model: RoleModel {
            model: "openai:test".parse().expect("a model id"),
            thinking: ReasoningLevel::Low,
        },
        routing: None,
        tools: vec![Arc::new(Lookup)],
        decisions: vec![pick()],
        task: "Pick one.".to_owned(),
        context: None,
        watch: None,
        gate: None,
    }
}

async fn run(script: Script) -> Result<Colour, Failure> {
    subagents(script).run("alice", spec()).await
}

#[tokio::test]
async fn a_run_returns_the_decision_it_called() {
    assert_eq!(run(Script::Decides).await, Ok(Colour("red".to_owned())));
    assert_eq!(
        run(Script::RetriesBadArguments).await,
        Ok(Colour("red".to_owned()))
    );
}

#[tokio::test]
async fn a_run_stops_at_its_token_budget_and_turn_limit() {
    assert!(matches!(
        run(Script::Expensive).await,
        Err(Failure::OverBudget { budget: 1000, .. })
    ));
    assert!(matches!(
        run(Script::Loops).await,
        Err(Failure::Failed(_) | Failure::Repeating)
    ));
}

#[tokio::test]
async fn a_cancelled_run_stops() {
    let watch = Watch::default();
    watch.cancel.cancel();
    let spec = Spec {
        watch: Some(watch),
        ..spec()
    };
    assert_eq!(
        subagents(Script::Loops).run("alice", spec).await,
        Err(Failure::Cancelled)
    );
}
