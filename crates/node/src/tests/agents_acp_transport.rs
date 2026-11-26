use super::*;
use futures::StreamExt;

#[tokio::test]
async fn an_oversized_message_is_reported_and_skipped() {
    let input = format!(
        "npm warn deprecated\n{{\"a\":\"{}\"}}\n{{\"b\":1}}\n",
        "x".repeat(64)
    );
    let (oversized, mut reports) = mpsc::channel(1);
    let read: Vec<String> = messages(std::io::Cursor::new(input.into_bytes()), 32, oversized)
        .map(|line| line.unwrap())
        .collect()
        .await;
    assert_eq!(read, [r#"{"b":1}"#]);
    assert_eq!(reports.recv().await, Some(72));
}
