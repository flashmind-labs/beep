use super::*;
use tokio::io::AsyncWriteExt;

/// `mesh join` checks that a node listens, waits for its socket, then
/// connects to send the ticket: three connections in a row, each of
/// which may find the node between two pipe instances.
#[tokio::test]
async fn connections_in_a_row_each_reach_the_node() {
    let socket = std::env::temp_dir().join(format!("beep-pipe-test-{}", std::process::id()));
    let mut listener = Listener::bind(&socket).expect("the pipe is made");
    tokio::spawn(async move {
        while let Ok(stream) = listener.accept().await {
            drop(stream);
        }
    });
    assert!(is_listening(&socket));
    let mut first = connect(&socket)
        .await
        .expect("the second connection reaches the node");
    first.shutdown().await.ok();
    connect(&socket)
        .await
        .expect("the third connection reaches the node");
}
