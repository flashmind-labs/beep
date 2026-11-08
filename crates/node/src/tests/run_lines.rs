use super::*;

#[tokio::test]
async fn splits_a_line_longer_than_a_piece() {
    let mut output = vec![b'x'; MAX_PIECE + 10];
    output.extend_from_slice(b"\nnext\n");
    let mut reader = &output[..];
    let mut buf = Vec::new();
    assert_eq!(
        read_piece(&mut reader, &mut buf).await.ok(),
        Some(MAX_PIECE)
    );
    buf.clear();
    assert_eq!(read_piece(&mut reader, &mut buf).await.ok(), Some(11));
    assert_eq!(buf.last(), Some(&b'\n'));
    buf.clear();
    read_piece(&mut reader, &mut buf)
        .await
        .expect("reading a slice cannot fail");
    assert_eq!(buf, b"next\n");
    buf.clear();
    assert_eq!(read_piece(&mut reader, &mut buf).await.ok(), Some(0));
}
