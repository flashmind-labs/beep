use super::*;
use tokio::io::BufReader;

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
