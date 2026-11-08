use super::*;
use flashmind::core::CancellationToken;

#[tokio::test]
async fn exec_says_when_no_desktop_is_online() {
    let tool = ExecTool {
        desktops: relay::Desktops::default(),
        user: "u".to_owned(),
    };
    let cancel = CancellationToken::new();
    let result = tool
        .execute(ToolContext::new(
            "call",
            json!({ "command": "ls" }),
            None,
            &cancel,
        ))
        .await
        .expect("exec answers");
    assert!(!result.is_success());
    assert_eq!(result.output(), "No desktop is online to run this.");
}
