use super::*;
use beep_proto::node::device::TouchPhase;

fn frame(n: u8) -> Frame {
    Frame {
        jpeg: vec![n],
        width: 2,
        height: 4,
    }
}

#[test]
fn only_the_newest_frame_waits_while_the_window_draws() {
    let start = Instant::now();
    let mut screen = Screen::default();

    assert_eq!(screen.arrived(frame(1), start), Some(frame(1)));
    assert_eq!(screen.arrived(frame(2), start), None);
    assert_eq!(screen.arrived(frame(3), start), None);
    assert_eq!(screen.drawn(start), Some(frame(3)));
    assert_eq!(screen.drawn(start), None);
    assert_eq!(screen.arrived(frame(4), start), Some(frame(4)));
}

#[test]
fn a_frame_goes_anyway_once_the_window_stalls() {
    let start = Instant::now();
    let mut screen = Screen::default();

    assert!(screen.arrived(frame(1), start).is_some());
    assert_eq!(screen.arrived(frame(2), start + DRAW_STALL), Some(frame(2)));
}

#[test]
fn a_frame_goes_as_its_size_then_the_jpeg() {
    let InvokeResponseBody::Raw(bytes) = ScreenEvent::Frame(frame(9)).body().unwrap() else {
        panic!("a frame goes raw");
    };
    assert_eq!(bytes, [2, 0, 0, 0, 4, 0, 0, 0, 9]);
}

#[tokio::test]
async fn frames_and_acknowledgements_run_while_input_is_blocked() {
    let attached = Attached::default();
    let view = Uuid::new_v4();
    let (input, up) = mpsc::channel(UP_QUEUE);
    let (drawn, draws) = watch::channel(());
    let controls = DeviceControls::new(input, drawn);
    let reader = InputReader::new(up, controls.overflow_up.clone(), controls.wake.clone());
    attached.follow(view, "window", controls);
    let input = attached.get(&view, "device").unwrap().input;
    let text = || Input::Text {
        text: "a".repeat(MAX_TEXT),
    };

    for _ in 0..UP_QUEUE {
        input.try_send(text()).unwrap();
    }

    let link = beep_proto::node::local_link();
    let (tx, rx) = link.caller;
    // Keep the other direction alive without reading it, so input backs up.
    let (mut node_tx, _node_rx) = link.node;
    let (shown, mut frames) = mpsc::unbounded_channel();
    let following = follow_screen(
        tx,
        rx,
        reader,
        draws,
        |event| {
            shown.send(event).unwrap();
            Ok(())
        },
        "Connection lost.",
    );
    tokio::pin!(following);
    assert!(futures::poll!(&mut following).is_pending());

    while input.try_send(text()).is_ok() {}
    assert_eq!(input.capacity(), 0);

    node_tx.send(&Reply::Frame(frame(1))).await.unwrap();
    assert!(futures::poll!(&mut following).is_pending());
    assert_eq!(frames.try_recv().unwrap(), ScreenEvent::Frame(frame(1)));

    node_tx.send(&Reply::Frame(frame(2))).await.unwrap();
    assert!(futures::poll!(&mut following).is_pending());
    assert!(frames.try_recv().is_err());

    attached
        .get(&view, "device")
        .unwrap()
        .drawn
        .send_replace(());
    assert!(futures::poll!(&mut following).is_pending());
    assert_eq!(frames.try_recv().unwrap(), ScreenEvent::Frame(frame(2)));
    assert_eq!(input.capacity(), 0);

    // An in-flight input sender must not keep the view alive after detach.
    attached.detach(&view);
    assert!(matches!(
        futures::poll!(&mut following),
        std::task::Poll::Ready(Ok(()))
    ));
}

#[test]
fn touches_stay_on_the_screen() {
    let touch = |x, y| Input::Touch {
        phase: TouchPhase::Move,
        x,
        y,
    };
    assert_eq!(checked(touch(-0.5, 1.5)), [touch(0.0, 1.0)]);
    assert!(checked(touch(f32::NAN, 0.5)).is_empty());
}

#[test]
fn long_text_goes_in_pieces_on_character_boundaries() {
    assert_eq!(pieces("abcdé", 5), ["abcd", "é"]);
    assert_eq!(pieces("", 5), Vec::<&str>::new());
    assert!(
        checked(Input::Text {
            text: String::new()
        })
        .is_empty()
    );
}

#[test]
fn input_reads_from_the_window_json() {
    let input: Input = serde_json::from_str(r#"{"Button":{"button":"Home"}}"#).unwrap();
    assert_eq!(
        input,
        Input::Button {
            button: beep_proto::node::device::Button::Home
        }
    );
}

#[tokio::test]
async fn video_preserves_every_chunk_and_does_not_block_input_while_waiting_for_the_viewer() {
    use device_video::{Reply as VideoReply, Request as VideoRequest};
    let link = beep_proto::node::local_link();
    let (tx, rx) = link.caller;
    let (mut node_tx, mut node_rx) = link.node;
    let (input, up) = mpsc::channel(UP_QUEUE);
    let (drawn, draws) = watch::channel(());
    let (shown, mut events) = mpsc::unbounded_channel();
    let following = follow_video(tx, rx, InputReader::from_channel(up), draws, |event| {
        shown.send(event).unwrap();
        Ok(())
    });
    tokio::pin!(following);

    node_tx
        .send(&VideoReply::Started { fps: 60 })
        .await
        .unwrap();
    assert!(futures::poll!(&mut following).is_pending());
    assert_eq!(events.try_recv().unwrap(), ScreenEvent::Video { fps: 60 });
    for n in [1, 2] {
        node_tx
            .send(&VideoReply::Data {
                data: vec![n],
                rotation: Rotation::Upright,
            })
            .await
            .unwrap();
    }
    assert!(futures::poll!(&mut following).is_pending());
    assert!(
        events.try_recv().is_err(),
        "unacknowledged bytes cannot be skipped"
    );

    let key = Input::Button {
        button: beep_proto::node::device::Button::Home,
    };
    input.send(key.clone()).await.unwrap();
    assert!(futures::poll!(&mut following).is_pending());
    assert_eq!(
        node_rx.recv::<VideoRequest>().await.unwrap(),
        Some(VideoRequest::Input { input: key })
    );

    for n in [1, 2] {
        drawn.send_replace(());
        assert!(futures::poll!(&mut following).is_pending());
        assert_eq!(
            events.try_recv().unwrap(),
            ScreenEvent::VideoData {
                data: vec![n],
                rotation: Rotation::Upright
            }
        );
    }
    drop(drawn);
    assert!(matches!(
        futures::poll!(&mut following),
        std::task::Poll::Ready(Ok(()))
    ));
}

#[test]
fn encoded_ipc_has_a_distinct_header_and_preserves_the_native_bytes() {
    let InvokeResponseBody::Raw(bytes) = ScreenEvent::VideoData {
        data: vec![0, 0, 1, 0x65],
        rotation: Rotation::Left,
    }
    .body()
    .unwrap() else {
        panic!("video is binary");
    };
    assert_eq!(&bytes[..4], &VIDEO_TAG.to_le_bytes());
    assert_eq!(&bytes[4..8], &1u32.to_le_bytes());
    assert_eq!(&bytes[8..], &[0, 0, 1, 0x65]);
}

#[tokio::test]
async fn consecutive_touch_moves_coalesce_without_dropping_touch_up() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(Input::Touch {
        phase: TouchPhase::Down,
        x: 0.1,
        y: 0.1,
    })
    .await
    .unwrap();
    tx.send(Input::Touch {
        phase: TouchPhase::Move,
        x: 0.2,
        y: 0.2,
    })
    .await
    .unwrap();
    tx.send(Input::Touch {
        phase: TouchPhase::Move,
        x: 0.4,
        y: 0.5,
    })
    .await
    .unwrap();
    tx.send(Input::Touch {
        phase: TouchPhase::Move,
        x: 0.8,
        y: 0.9,
    })
    .await
    .unwrap();
    tx.send(Input::Touch {
        phase: TouchPhase::Up,
        x: 0.8,
        y: 0.9,
    })
    .await
    .unwrap();
    drop(tx);

    let mut reader = InputReader::from_channel(rx);
    assert_eq!(
        reader.next().await,
        Some(Input::Touch {
            phase: TouchPhase::Down,
            x: 0.1,
            y: 0.1,
        })
    );
    assert_eq!(
        reader.next().await,
        Some(Input::Touch {
            phase: TouchPhase::Move,
            x: 0.8,
            y: 0.9,
        })
    );
    assert_eq!(
        reader.next().await,
        Some(Input::Touch {
            phase: TouchPhase::Up,
            x: 0.8,
            y: 0.9,
        })
    );
    assert_eq!(reader.next().await, None);
}

#[tokio::test]
async fn overflow_touch_up_waits_behind_queued_touch_down_without_blocking() {
    let (tx, rx) = mpsc::channel(2);
    let (drawn, _draws) = watch::channel(());
    let controls = DeviceControls::new(tx, drawn);
    let mut reader = InputReader::new(rx, controls.overflow_up.clone(), controls.wake.clone());

    send_device_input(
        &controls,
        Input::Touch {
            phase: TouchPhase::Down,
            x: 0.2,
            y: 0.3,
        },
    )
    .await
    .unwrap();
    send_device_input(
        &controls,
        Input::Button {
            button: beep_proto::node::device::Button::Home,
        },
    )
    .await
    .unwrap();
    assert_eq!(controls.input.capacity(), 0);

    // Queue is full, yet TouchPhase::Up returns immediately and delivers after queued items.
    send_device_input(
        &controls,
        Input::Touch {
            phase: TouchPhase::Up,
            x: 0.2,
            y: 0.3,
        },
    )
    .await
    .unwrap();

    assert_eq!(
        reader.next().await,
        Some(Input::Touch {
            phase: TouchPhase::Down,
            x: 0.2,
            y: 0.3,
        })
    );
    assert_eq!(
        reader.next().await,
        Some(Input::Button {
            button: beep_proto::node::device::Button::Home,
        })
    );
    assert_eq!(
        reader.next().await,
        Some(Input::Touch {
            phase: TouchPhase::Up,
            x: 0.2,
            y: 0.3,
        })
    );
}

#[tokio::test(start_paused = true)]
async fn this_machines_cold_list_is_capped_by_the_first_wait() {
    let machine = iroh::SecretKey::from_bytes(&[7; 32]).public();
    let devices = Projected::new(
        Duration::from_secs(10),
        Duration::from_millis(600),
        DEVICE_LIST_LIMIT,
    );
    let started = Instant::now();

    let read = read_devices(
        devices,
        machine,
        || {},
        || async { std::future::pending::<Result<Vec<Device>>>().await },
    )
    .await;

    assert!(read.is_none());
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn a_list_still_loading_is_not_an_empty_machine() {
    let machine = iroh::SecretKey::from_bytes(&[7; 32]).public();

    let loading = DeviceMachine::new(machine, "box".to_owned(), None);
    let empty = DeviceMachine::new(machine, "box".to_owned(), Some(Ok(Vec::new())));

    assert!(loading.loading && loading.devices.is_empty() && loading.error.is_none());
    assert!(!empty.loading);
}

#[tokio::test(start_paused = true)]
async fn a_list_that_fails_after_the_first_wait_tells_the_window() {
    let machine = iroh::SecretKey::from_bytes(&[7; 32]).public();
    let devices = Projected::new(
        Duration::from_secs(10),
        Duration::from_millis(600),
        DEVICE_LIST_LIMIT,
    );
    let (fail, failing) = tokio::sync::oneshot::channel::<()>();
    let (tell, told) = tokio::sync::oneshot::channel::<()>();
    let slow = || async {
        let _ = failing.await;
        Err(DesktopError::Other(anyhow::anyhow!("no answer")))
    };

    assert!(
        read_devices(
            devices.clone(),
            machine,
            move || {
                let _ = tell.send(());
            },
            slow
        )
        .await
        .is_none()
    );
    fail.send(()).unwrap();
    told.await.expect("the window is told");

    let again = read_devices(devices, machine, || {}, || async { Ok(Vec::new()) }).await;
    assert!(matches!(again, Some(Err(_))));
}

#[tokio::test(start_paused = true)]
async fn a_machine_that_never_answers_ends_as_a_failed_list() {
    let machine = iroh::SecretKey::from_bytes(&[7; 32]).public();
    let held = Projected::new(
        Duration::from_secs(10),
        Duration::from_millis(600),
        DEVICE_LIST_LIMIT,
    );

    // Bounded, so a listing that never gives up fails here instead of hanging.
    let listed = tokio::time::timeout(
        DEVICE_LIST_LIMIT + Duration::from_secs(1),
        listing(held.clone(), machine, || {
            std::future::pending::<Result<Vec<Device>>>()
        }),
    )
    .await
    .expect("the listing gives up within its limit");
    assert!(matches!(listed, Ok(Listing::Failed(_))));

    held.put(machine, Listing::Failed("old".to_owned()));
    let again = listing(held, machine, || async {
        Err::<Vec<Device>, _>(DesktopError::Other(anyhow::anyhow!("newer")))
    })
    .await;
    assert!(matches!(again, Ok(Listing::Failed(text)) if text.contains("newer")));
}
