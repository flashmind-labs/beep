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
    let refusals = guard_tool_calls(&mut registry);
    let tool = registry.get("echo").expect("echo registered");
    let cancel = CancellationToken::new();
    let call = |args: Value| tool.execute(ToolContext::new("call", args, None, &cancel));

    assert!(call(json!({"q": "a"})).await.unwrap().is_success());
    assert!(call(json!({"q": "a"})).await.unwrap().is_success());
    assert!(!call(json!({"q": "a"})).await.unwrap().is_success());
    assert!(call(json!({"q": "b"})).await.unwrap().is_success());
    assert!(!refusals.exhausted());
    call(json!({"q": "a"})).await.unwrap();
    call(json!({"q": "a"})).await.unwrap();
    assert!(refusals.exhausted());
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
