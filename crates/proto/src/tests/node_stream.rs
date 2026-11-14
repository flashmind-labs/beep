use super::*;
use tokio::io::AsyncReadExt;
use tokio::time::{Instant, advance};
use tokio_util::codec::Encoder;

const IDLE: Duration = Duration::from_secs(30);

#[tokio::test(start_paused = true)]
async fn sending_keeps_running_while_bytes_move() {
    let (writer, mut reader) = duplex(16);
    let mut tx = LinkTx::new(Out::Local(writer));
    tx.set_idle_timeout(IDLE);
    let message = "a".repeat(128);
    let mut send = Box::pin(tx.send(&message));
    let started = Instant::now();
    let mut buffer = [0; 16];

    // Drain a little before each idle deadline, taking far longer than 30s.
    loop {
        if let Poll::Ready(result) = futures::poll!(&mut send) {
            result.unwrap();
            break;
        }

        advance(Duration::from_secs(20)).await;
        assert!(reader.read(&mut buffer).await.unwrap() > 0);
    }

    assert!(started.elapsed() > IDLE);
}

#[tokio::test(start_paused = true)]
async fn receiving_keeps_running_while_a_frame_arrives() {
    let (mut writer, reader) = duplex(64);
    let mut rx = LinkRx::new(In::Local(reader));
    rx.set_idle_timeout(IDLE);
    let mut frame = bytes::BytesMut::new();
    codec()
        .encode(
            Bytes::from(rmp_serde::to_vec(&"photo").unwrap()),
            &mut frame,
        )
        .unwrap();
    let mut receive = Box::pin(rx.recv::<String>());
    let started = Instant::now();

    for byte in frame {
        assert!(futures::poll!(&mut receive).is_pending());
        advance(Duration::from_secs(20)).await;
        writer.write_all(&[byte]).await.unwrap();
    }

    assert_eq!(receive.await.unwrap().as_deref(), Some("photo"));
    assert!(started.elapsed() > IDLE);
}

#[tokio::test(start_paused = true)]
async fn a_blocked_writer_times_out_after_30_seconds() {
    let (writer, _reader) = duplex(16);
    let mut tx = LinkTx::new(Out::Local(writer));
    tx.set_idle_timeout(IDLE);
    let started = Instant::now();
    let error = tx.send(&"a".repeat(128)).await.unwrap_err();

    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::TimedOut
    );
    assert_eq!(started.elapsed(), IDLE);
}

#[tokio::test(start_paused = true)]
async fn a_partial_frame_times_out_30_seconds_after_the_last_bytes() {
    let (mut writer, reader) = duplex(64);
    let mut rx = LinkRx::new(In::Local(reader));
    rx.set_idle_timeout(IDLE);
    let mut receive = Box::pin(rx.recv::<String>());
    assert!(futures::poll!(&mut receive).is_pending());
    advance(Duration::from_secs(20)).await;
    writer.write_all(&[0, 0]).await.unwrap();
    assert!(futures::poll!(&mut receive).is_pending());
    let last_progress = Instant::now();
    let error = receive.await.unwrap_err();

    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::TimedOut
    );
    assert_eq!(last_progress.elapsed(), IDLE);
}
