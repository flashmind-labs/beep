use super::*;
use crate::store::TestDb;
use flashmind::core::CancellationToken;

async fn call(tool: &dyn Tool, args: Value) -> ToolResult {
    let cancel = CancellationToken::new();
    tool.execute(ToolContext::new("call", args, None, &cancel))
        .await
        .expect("the store answers")
}

#[tokio::test]
async fn stores_then_recalls_and_a_reader_cannot_write() {
    let db = TestDb::new().await.expect("test database");
    let user = User::for_test(db.store().await.expect("store"), "alice");
    let mut registry = ToolRegistry::new();
    register(&mut registry, &user, "chat");
    let memory = registry.get(NAME).expect("memory registered").clone();

    let stored = call(
        memory.as_ref(),
        json!({"action": "store", "content": "Alice prefers tea."}),
    )
    .await;
    assert!(stored.is_success(), "{}", stored.output());
    let recalled = call(memory.as_ref(), json!({"action": "recall", "query": ""})).await;
    assert!(recalled.output().contains("Alice prefers tea."));

    let reader = recall_tool(&user);
    assert_eq!(
        reader.parameters()["properties"]["action"]["enum"],
        json!(["recall", "history"])
    );
    let refused = call(
        reader.as_ref(),
        json!({"action": "store", "content": "Alice prefers coffee."}),
    )
    .await;
    assert!(!refused.is_success());
    let recalled = call(reader.as_ref(), json!({"action": "recall", "query": ""})).await;
    assert!(!recalled.output().contains("coffee"));
}
