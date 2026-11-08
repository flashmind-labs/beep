use super::*;
use futures::StreamExt;

#[tokio::test]
async fn lines_over_the_cap_are_dropped_whole() {
    let input = format!("short\n{}\nafter\r\nlast", "x".repeat(100));
    // A small buffer, so the long line spans many reads.
    let reader = BufReader::with_capacity(8, input.as_bytes());
    let mut lines = LineReader::new(reader, 10);
    let mut read = Vec::new();
    while let Some(line) = lines.next().await.unwrap() {
        read.push(line);
    }
    assert_eq!(
        read,
        [
            Line::Text("short".into()),
            Line::TooLong(100),
            Line::Text("after".into()),
            Line::Text("last".into()),
        ]
    );
}

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
