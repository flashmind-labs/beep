use super::*;
use crate::{account::user::UserKey, store::TestDb};
use flashmind::core::CancellationToken;

async fn call(tool: &SkillTool, args: Value) -> ToolResult {
    let cancel = CancellationToken::new();
    tool.execute(ToolContext::new("call", args, None, &cancel))
        .await
        .expect("the store answers")
}

#[tokio::test]
async fn a_group_only_views_skills() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("store");
    let tool = SkillTool {
        store,
        owner: SkillOwner::User(UserKey::for_test("alice")),
        writes: false,
    };
    assert_eq!(
        tool.parameters()["properties"]["action"]["enum"],
        json!(["view"])
    );
    let refused = call(
        &tool,
        json!({"action": "create", "name": "x", "description": "y", "body": "z"}),
    )
    .await;
    assert!(!refused.is_success());
}
