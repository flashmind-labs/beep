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

#[tokio::test]
async fn zstd_compresses_large_frames_and_skips_small_and_incompressible() {
    let pair = local_link();
    let (mut tx, mut rx) = (pair.caller.0, pair.node.1);
    tx.enable_zstd();
    rx.enable_zstd();

    let small = "short".to_owned();
    let large = "repeated transcript text ".repeat(400);
    let mut randomish = Vec::with_capacity(2048);
    let mut state = 0x1234_5678_9abc_def0_u64;
    for _ in 0..2048 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        randomish.push((state >> 56) as u8);
    }

    tx.send(&small).await.unwrap();
    tx.send(&large).await.unwrap();
    tx.send(&serde_bytes::ByteBuf::from(randomish.clone()))
        .await
        .unwrap();

    assert_eq!(rx.recv::<String>().await.unwrap(), Some(small));
    assert_eq!(rx.recv::<String>().await.unwrap(), Some(large.clone()));
    assert_eq!(
        rx.recv::<serde_bytes::ByteBuf>().await.unwrap(),
        Some(serde_bytes::ByteBuf::from(randomish))
    );

    // Inspect raw framing bytes on a duplex pipe to confirm compression behavior.
    let (writer, reader) = duplex(64 * 1024);
    let mut raw_tx = LinkTx::new(Out::Local(writer));
    raw_tx.enable_zstd();
    let mut raw_rx = FramedRead::new(reader, codec());

    raw_tx.send(&"tiny").await.unwrap();
    let small_frame = raw_rx.next().await.unwrap().unwrap();
    assert_eq!(small_frame[0], FRAME_RAW);

    raw_tx.send(&large).await.unwrap();
    let large_frame = raw_rx.next().await.unwrap().unwrap();
    assert_eq!(large_frame[0], FRAME_ZSTD);
    assert!(
        large_frame.len() < large.len() / 5,
        "expected strong zstd compression, got {} for {}",
        large_frame.len(),
        large.len()
    );
}

#[tokio::test]
async fn zstd_rejects_decompressed_frames_over_max_frame() {
    let (writer, reader) = duplex(64 * 1024);
    let mut tx = LinkTx::new(Out::Local(writer));
    let mut rx = LinkRx::new(In::Local(reader));
    tx.enable_zstd();
    rx.enable_zstd();
    rx.set_max_frame(512);

    let bomb = "A".repeat(4096);
    tx.send(&bomb).await.unwrap();
    let err = rx.recv::<String>().await.unwrap_err();
    assert!(
        format!("{err:#}").contains("exceeds frame limit"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn open_and_subscriptions_handshake_stays_compatible_with_legacy_peers() {
    use crate::node::{Open, commands, subscriptions};

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct OldOpen {
        protocol: String,
    }

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct OldSubOpen {}

    let new_open = Open {
        protocol: "agents/1".into(),
        compress: true,
    };
    let bytes = rmp_serde::to_vec(&new_open).unwrap();
    assert_eq!(
        lenient::from_slice::<OldOpen>(&bytes).unwrap(),
        OldOpen {
            protocol: "agents/1".into(),
        }
    );

    let old_bytes = rmp_serde::to_vec(&OldOpen {
        protocol: "agents/1".into(),
    })
    .unwrap();
    assert_eq!(
        lenient::from_slice::<Open>(&old_bytes).unwrap(),
        Open {
            protocol: "agents/1".into(),
            compress: false,
        }
    );

    let sub_bytes = rmp_serde::to_vec(&subscriptions::Open { compress: true }).unwrap();
    assert_eq!(
        lenient::from_slice::<OldSubOpen>(&sub_bytes).unwrap(),
        OldSubOpen {}
    );

    let old_sub_bytes = rmp_serde::to_vec(&OldSubOpen {}).unwrap();
    assert_eq!(
        lenient::from_slice::<subscriptions::Open>(&old_sub_bytes).unwrap(),
        subscriptions::Open { compress: false }
    );

    let cmd_bytes = rmp_serde::to_vec(&commands::Open::Session {
        protocol: "agents/1".into(),
        compress: true,
    })
    .unwrap();
    assert_eq!(
        lenient::from_slice::<commands::Open>(&cmd_bytes).unwrap(),
        commands::Open::Session {
            protocol: "agents/1".into(),
            compress: true,
        }
    );
}
