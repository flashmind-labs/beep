use super::*;
use super::screen::{FRAME_GAP, Screen, Turn, Turned, follow, point};
use std::sync::{Arc, atomic::{AtomicU8, Ordering}};
use futures::StreamExt;
use tokio::{sync::watch, time::Instant};
use beep_proto::node::device::Frame;
use beep_proto::node::device::{Button, DeviceState, Key, TouchPhase};
use std::time::Duration;
use tokio::time::timeout;

#[test]
fn a_touch_lands_on_the_screen_even_outside_it() {
    assert_eq!(point(0.5, 0.25, 1000.0, 2000.0), (500.0, 500.0));
    assert_eq!(point(-0.2, 1.5, 1000.0, 2000.0), (0.0, 2000.0));
    assert_eq!(point(f32::NAN, 1.0, 1000.0, 2000.0), (0.0, 2000.0));
}

#[test]
fn a_touch_on_a_turned_frame_lands_where_it_shows_on_the_upright_screen() {
    // The upright screen's top left corner, wherever a turned frame shows it.
    assert_eq!(Turn::Upright.upright(0.0, 0.0), (0.0, 0.0));
    assert_eq!(Turn::Left.upright(0.0, 1.0), (0.0, 0.0));
    assert_eq!(Turn::UpsideDown.upright(1.0, 1.0), (0.0, 0.0));
    assert_eq!(Turn::Right.upright(1.0, 0.0), (0.0, 0.0));

    let turned = Turned::default();
    assert_eq!(turned.get(), Turn::Upright);
    turned.set(Turn::Right);
    assert_eq!(turned.clone().get(), Turn::Right);
}

#[tokio::test]
async fn input_with_nothing_attached_fails_and_the_stream_goes_on() {
    let link = beep_proto::node::local_link();
    let (node_tx, node_rx) = link.node;
    let (mut tx, mut rx) = link.caller;
    let serving = tokio::spawn(serve(node_tx, node_rx));

    let input = Input::Text {
        text: "hi".to_owned(),
    };
    tx.send(&Request::Input { input }).await.expect("sent");
    let reply: Reply = rx.recv().await.expect("read").expect("a reply");
    assert!(matches!(reply, Reply::Failed { .. }), "{reply:?}");

    drop(tx);
    serving.await.expect("joined").expect("served");
}

/// Answers the stream's requests, none of which names a path.
async fn serve(tx: LinkTx, rx: LinkRx) -> Result<()> {
    let mut stream = Served::new(tx, rx);

    while let Some(request) = stream.next().await? {
        assert!(named(&request).is_none());
        let Some(more) = stream.answer(request, None).await? else {
            break;
        };
        stream = more;
    }

    Ok(())
}

/// One request on a stream of its own, and its reply.
async fn ask(request: Request) -> Reply {
    let link = beep_proto::node::local_link();
    let (node_tx, node_rx) = link.node;
    let (mut tx, mut rx) = link.caller;
    let _serving = AbortOnDropHandle::new(tokio::spawn(serve(node_tx, node_rx)));
    tx.send(&request).await.expect("sent");
    rx.recv().await.expect("read").expect("a reply")
}

/// The inputs of a step such as `tap:0.5,0.5`, `drag:0.5,0.9,0.5,0.2`,
/// `text:hello`, `key:return`, `home`, `back` or `rotate`.
fn step(step: &str) -> Vec<Input> {
    let (name, args) = step.split_once(':').unwrap_or((step, ""));
    let numbers: Vec<f32> = args.split(',').filter_map(|n| n.parse().ok()).collect();
    let touch = |phase, at: &[f32]| Input::Touch {
        phase,
        x: at[0],
        y: at[1],
    };
    let button = |button| vec![Input::Button { button }];

    match name {
        "tap" => vec![
            touch(TouchPhase::Down, &numbers),
            touch(TouchPhase::Up, &numbers),
        ],
        "drag" => {
            let (from, to) = numbers.split_at(2);
            let mut inputs = vec![touch(TouchPhase::Down, from)];
            inputs.extend((1..=10).map(|i| {
                let t = i as f32 / 10.0;
                let at = [
                    from[0] + (to[0] - from[0]) * t,
                    from[1] + (to[1] - from[1]) * t,
                ];
                touch(TouchPhase::Move, &at)
            }));
            inputs.push(touch(TouchPhase::Up, to));
            inputs
        }
        "text" => vec![Input::Text {
            text: args.to_owned(),
        }],
        "key" => vec![Input::Key {
            key: match args {
                "return" => Key::Return,
                "delete" => Key::Delete,
                other => panic!("unknown key {other}"),
            },
        }],
        "home" => button(Button::Home),
        "back" => button(Button::Back),
        "rotate" => button(Button::Rotate),
        other => panic!("unknown step {other}"),
    }
}

/// Lists, boots when off, attaches and takes a frame of the device
/// `BEEP_DEVICE_TEST` names (an AVD name or a simulator UDID), then takes
/// the steps `BEEP_DEVICE_TEST_STEPS` lists (see `step`, split by `;`, a
/// tap in the middle when unset) and shuts down what it booted. With
/// `BEEP_DEVICE_TEST_OUT` set, the newest frame after each step goes there
/// as a JPEG.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs an emulator or simulator; set BEEP_DEVICE_TEST"]
async fn a_real_device_shows_its_screen_and_takes_input() {
    let id = DeviceId(std::env::var("BEEP_DEVICE_TEST").expect("BEEP_DEVICE_TEST"));
    let out = std::env::var_os("BEEP_DEVICE_TEST_OUT").map(std::path::PathBuf::from);
    let steps = std::env::var("BEEP_DEVICE_TEST_STEPS").unwrap_or_else(|_| "tap:0.5,0.5".into());

    let Reply::Devices { devices } = ask(Request::List {}).await else {
        panic!("no device list");
    };
    eprintln!("{devices:#?}");
    let device = devices
        .iter()
        .find(|device| device.id == id)
        .expect("the device is listed");
    let booted = device.state == DeviceState::Off;

    if booted {
        let reply = ask(Request::Boot { id: id.clone() }).await;
        assert_eq!(reply, Reply::Done {});
    }

    let link = beep_proto::node::local_link();
    let (node_tx, node_rx) = link.node;
    let (mut tx, mut rx) = link.caller;
    let _serving = AbortOnDropHandle::new(tokio::spawn(serve(node_tx, node_rx)));
    let attach = Request::Attach {
        id: id.clone(),
        max_side: 800,
    };
    tx.send(&attach).await.expect("sent");
    let first = timeout(Duration::from_secs(240), rx.recv::<Reply>())
        .await
        .expect("a frame in time")
        .expect("read")
        .expect("a reply");
    let Reply::Frame(frame) = first else {
        panic!("not a frame: {first:?}");
    };
    assert!(frame.width.max(frame.height) <= 800, "{frame:?}");
    let save = |name: &str, frame: &Frame| {
        eprintln!("{name}: {frame:?}");

        if let Some(out) = &out {
            std::fs::write(out.join(format!("{name}.jpg")), &frame.jpeg).expect("saved");
        }
    };
    save("0-attached", &frame);
    let mut newest = frame;

    for (n, name) in steps.split(';').enumerate() {
        // Spaced as a hand moves, about one input a frame.
        for input in step(name) {
            tx.send(&Request::Input { input }).await.expect("sent");
            tokio::time::sleep(Duration::from_millis(16)).await;
        }

        let settled = tokio::time::sleep(Duration::from_secs(3));
        tokio::pin!(settled);

        loop {
            tokio::select! {
                () = &mut settled => break,
                reply = rx.recv::<Reply>() => match reply.expect("read").expect("a reply") {
                    Reply::Frame(frame) => newest = frame,
                    other => panic!("not a frame: {other:?}"),
                },
            }
        }

        let name = name.split(':').next().unwrap_or(name);
        save(&format!("{}-{name}", n + 1), &newest);
    }

    drop(tx);
    let logs = ask(Request::Logs {
        id: id.clone(),
        lines: 5,
        filter: String::new(),
    })
    .await;
    let Reply::Logs { text } = logs else {
        panic!("no logs: {logs:?}");
    };
    eprintln!("logs:\n{text}");
    assert!(!text.is_empty() && text.lines().count() <= 5, "{text}");
    let ui = ask(Request::Ui { id: id.clone() }).await;
    let Reply::Ui { elements } = ui else {
        panic!("no screen contents: {ui:?}");
    };
    eprintln!(
        "ui: {} elements, first {:?}",
        elements.len(),
        elements.first()
    );
    assert!(!elements.is_empty());

    if booted {
        let reply = ask(Request::Shutdown { id }).await;
        assert_eq!(reply, Reply::Done {});
    }
}

#[test]
fn logs_keep_the_newest_matching_lines() {
    let text = "a Beep start\nb other\nc beep tap\nd BEEP done\n";
    assert_eq!(newest_lines(text, "beep", 2), "c beep tap\nd BEEP done");
    assert_eq!(newest_lines(text, "", 1), "d BEEP done");
    assert_eq!(newest_lines(text, "missing", 5), "");
}

fn streamed_frame(value: u8) -> Frame {
    Frame {
        jpeg: vec![value],
        width: 1,
        height: 1,
    }
}

#[tokio::test]
async fn blocked_video_writes_do_not_delay_input_and_only_the_newest_frame_waits() {
    let link = beep_proto::node::local_link();
    let (node_tx, mut node_rx) = link.node;
    let (mut tx, mut rx) = link.caller;
    let (screen, shown) = watch::channel(Screen::Starting);
    let (taken, mut inputs) = tokio::sync::mpsc::unbounded_channel();
    let inputs_forwarded = async {
        while let Some(Request::Input { input }) = node_rx.recv::<Request>().await? {
            taken.send(input).expect("input recorded");
        }

        Ok(())
    };
    let mut serving = std::pin::pin!(stream(node_tx, shown, inputs_forwarded));

    // Larger than the local link's buffer, so sending cannot finish until read.
    let large = Frame {
        jpeg: vec![1; 128 * 1024],
        width: 1,
        height: 1,
    };
    screen.send_replace(Screen::Frame(large.clone()));
    assert!(futures::poll!(serving.as_mut()).is_pending());

    let input = Input::Button {
        button: Button::Home,
    };
    tx.send(&Request::Input {
        input: input.clone(),
    })
    .await
    .expect("sent");
    assert!(futures::poll!(serving.as_mut()).is_pending());
    assert_eq!(inputs.try_recv().expect("input bypassed video"), input);

    screen.send_replace(Screen::Frame(streamed_frame(2)));
    screen.send_replace(Screen::Frame(streamed_frame(3)));
    for expected in [large, streamed_frame(3)] {
        let reply = tokio::select! {
            result = serving.as_mut() => panic!("view ended: {result:?}"),
            reply = rx.recv::<Reply>() => reply.expect("read").expect("frame"),
        };
        assert_eq!(reply, Reply::Frame(expected));
    }

    // Closing input also cancels a video write that cannot finish.
    screen.send_replace(Screen::Frame(Frame {
        jpeg: vec![4; 128 * 1024],
        width: 1,
        height: 1,
    }));
    assert!(futures::poll!(serving.as_mut()).is_pending());
    drop(tx);
    serving.await.expect("detached");
}

#[tokio::test]
async fn slow_input_does_not_delay_video_and_ends_with_the_screen() {
    let link = beep_proto::node::local_link();
    let (node_tx, mut node_rx) = link.node;
    let (mut tx, mut rx) = link.caller;
    let (screen, shown) = watch::channel(Screen::Starting);
    let (taken, mut inputs) = tokio::sync::mpsc::unbounded_channel();
    let inputs_forwarded = async {
        while let Some(Request::Input { input }) = node_rx.recv::<Request>().await? {
            taken.send(input).expect("input recorded");
            std::future::pending::<()>().await;
        }

        Ok(())
    };
    let mut serving = std::pin::pin!(stream(node_tx, shown, inputs_forwarded));

    let input = Input::Button {
        button: Button::Home,
    };
    tx.send(&Request::Input {
        input: input.clone(),
    })
    .await
    .expect("sent");
    assert!(futures::poll!(serving.as_mut()).is_pending());
    assert_eq!(inputs.try_recv().expect("input started"), input);

    screen.send_replace(Screen::Frame(streamed_frame(1)));
    let reply = tokio::select! {
        result = serving.as_mut() => panic!("view ended: {result:?}"),
        reply = rx.recv::<Reply>() => reply.expect("read").expect("frame"),
    };
    assert_eq!(reply, Reply::Frame(streamed_frame(1)));

    let reason = "The device stopped.".to_owned();
    screen.send_replace(Screen::Ended(reason.clone()));
    serving.await.expect("ended while input is blocked");
    assert_eq!(
        rx.recv::<Reply>().await.expect("read"),
        Some(Reply::Ended { reason })
    );
}

#[tokio::test(start_paused = true)]
async fn conversion_time_is_included_in_the_frame_budget_without_catch_up_bursts() {
    use tokio::time::{advance, interval};
    use tokio_stream::wrappers::IntervalStream;

    let source = IntervalStream::new(interval(Duration::from_millis(1))).map(|_| Ok(()));
    let (screen, mut shown) = watch::channel(Screen::Starting);
    let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
    let origin = Instant::now();
    let mut costs = [10, 50, 10].into_iter();
    let mut following = std::pin::pin!(follow(
        source,
        screen,
        async |()| {
            started
                .send(Instant::now().duration_since(origin))
                .expect("start recorded");
            tokio::time::sleep(Duration::from_millis(costs.next().unwrap_or(10))).await;
            Ok(Some(streamed_frame(1)))
        },
        "ended"
    ));

    assert!(futures::poll!(following.as_mut()).is_pending());
    assert_eq!(starts.try_recv().expect("first"), Duration::ZERO);
    advance(Duration::from_millis(10)).await;
    assert!(futures::poll!(following.as_mut()).is_pending());
    assert!(shown.has_changed().expect("screen open"));
    watched::read_and_update(&mut shown, |_| ());
    advance(Duration::from_millis(23)).await;
    assert!(futures::poll!(following.as_mut()).is_pending());
    assert_eq!(starts.try_recv().expect("second"), FRAME_GAP);

    // A slow conversion misses a deadline. The next start resets the budget.
    advance(Duration::from_millis(50)).await;
    assert!(futures::poll!(following.as_mut()).is_pending());
    assert_eq!(starts.try_recv().expect("third"), Duration::from_millis(83));
    assert!(shown.has_changed().expect("slow frame delivered"));
    watched::read_and_update(&mut shown, |_| ());
    advance(Duration::from_millis(10)).await;
    assert!(futures::poll!(following.as_mut()).is_pending());
    assert!(
        starts.try_recv().is_err(),
        "no immediate catch-up conversion"
    );
    assert!(
        !shown.has_changed().expect("screen open"),
        "no delivery burst after a slow frame"
    );
    advance(Duration::from_millis(23)).await;
    assert!(futures::poll!(following.as_mut()).is_pending());
    assert_eq!(
        starts.try_recv().expect("fourth"),
        Duration::from_millis(116)
    );
    assert!(
        shown
            .has_changed()
            .expect("next frame delivered on its deadline")
    );
}

fn device(state: DeviceState) -> Device {
    Device {
        id: DeviceId("emulator-5554".to_owned()),
        name: "Pixel".to_owned(),
        platform: Platform::Android,
        state,
        os: None,
        phone: false,
    }
}

/// A follower task over a device list the test sets, counting reads.
struct Followed {
    list: Arc<std::sync::Mutex<Vec<Device>>>,
    reads: Arc<AtomicU8>,
    changed: watch::Sender<()>,
    listed: &'static watch::Sender<Option<Vec<Device>>>,
    /// The stored list at each ping.
    pings: tokio::sync::mpsc::UnboundedReceiver<Option<Vec<Device>>>,
    task: AbortOnDropHandle<()>,
}

fn followed(first: Vec<Device>) -> Followed {
    let list = Arc::new(std::sync::Mutex::new(first));
    let reads = Arc::new(AtomicU8::new(0));
    let (changed, rings) = watch::channel(());
    let listed: &'static _ = Box::leak(Box::new(watch::Sender::new(None)));
    let (ping, pings) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(following_list(
        {
            let (list, reads) = (list.clone(), reads.clone());
            move || {
                reads.fetch_add(1, Ordering::Relaxed);
                let list = list.lock().unwrap().clone();
                async move { list }
            }
        },
        listed,
        rings,
        move || {
            let _ = ping.send(watched::read_sent(listed, Clone::clone));
        },
    ));

    Followed {
        list,
        reads,
        changed,
        listed,
        pings,
        task: AbortOnDropHandle::new(task),
    }
}

impl Followed {
    /// Waits for the first read to be stored.
    async fn started(&self) {
        let mut listed = self.listed.subscribe();
        watched::wait_for(&mut listed, Option::is_some)
            .await
            .expect("the list is stored");
    }
}

fn simulator() -> Device {
    Device {
        platform: Platform::Ios,
        id: DeviceId("sim-1".to_owned()),
        ..device(DeviceState::On)
    }
}

#[tokio::test(start_paused = true)]
async fn a_change_is_read_once_and_stored_before_it_is_pinged() {
    let mut follow = followed(vec![device(DeviceState::On)]);
    follow.started().await;
    *follow.list.lock().unwrap() = vec![device(DeviceState::Off)];
    follow.changed.send_replace(());

    // The ping finds the new list stored: nobody reads it again.
    assert_eq!(
        follow.pings.recv().await,
        Some(Some(vec![device(DeviceState::Off)]))
    );
    assert_eq!(follow.reads.load(Ordering::Relaxed), 2);
}

#[tokio::test(start_paused = true)]
async fn a_list_with_nothing_to_watch_is_never_read_on_a_timer() {
    let follow = followed(vec![device(DeviceState::On)]);
    follow.started().await;

    tokio::time::sleep(SIMULATORS_EVERY * 20).await;

    assert_eq!(follow.reads.load(Ordering::Relaxed), 1);
    assert!(!follow.task.is_finished());
}

#[tokio::test(start_paused = true)]
async fn a_device_that_finishes_booting_with_no_event_is_pinged() {
    let mut follow = followed(vec![device(DeviceState::Booting)]);

    // The first read sees it booting.
    follow.started().await;
    *follow.list.lock().unwrap() = vec![device(DeviceState::On)];

    assert_eq!(
        follow.pings.recv().await,
        Some(Some(vec![device(DeviceState::On)]))
    );
}

#[tokio::test(start_paused = true)]
async fn a_simulator_changed_outside_beep_is_read_while_it_is_listed() {
    let mut follow = followed(vec![simulator()]);
    follow.started().await;
    *follow.list.lock().unwrap() = Vec::new();

    assert_eq!(follow.pings.recv().await, Some(Some(Vec::new())));
}

#[tokio::test(start_paused = true)]
async fn a_shut_down_simulator_is_never_read_on_a_timer() {
    let follow = followed(vec![Device {
        state: DeviceState::Off,
        ..simulator()
    }]);
    follow.started().await;

    tokio::time::sleep(SIMULATORS_EVERY * 20).await;

    assert_eq!(follow.reads.load(Ordering::Relaxed), 1);
}

#[tokio::test(start_paused = true)]
async fn nothing_is_read_once_nobody_follows() {
    let follow = followed(vec![device(DeviceState::Booting)]);
    follow.started().await;
    tokio::time::sleep(BOOTING_EVERY * 2).await;
    let reads = follow.reads.clone();
    drop(follow);
    let before = reads.load(Ordering::Relaxed);
    tokio::time::sleep(SIMULATORS_EVERY * 10).await;

    assert!(before >= 2);
    assert_eq!(reads.load(Ordering::Relaxed), before);
}

#[test]
fn a_command_for_a_device_just_listed_does_not_read_every_platform_again() {
    use futures::FutureExt;

    let last = Mutex::new(vec![device(DeviceState::On)]);
    let id = last.lock().unwrap().first().unwrap().id.clone();
    let never = || std::future::pending::<Vec<Device>>();

    let found = find_on(&id, &last, never).now_or_never();

    assert_eq!(found.unwrap().unwrap().id, id);
}

#[test]
fn a_device_not_listed_on_is_read_again() {
    use futures::FutureExt;

    let last = Mutex::new(vec![device(DeviceState::Booting)]);
    let id = last.lock().unwrap().first().unwrap().id.clone();
    let came_up = vec![device(DeviceState::On)];

    let found = find_on(&id, &last, || std::future::ready(came_up)).now_or_never();

    assert_eq!(found.unwrap().unwrap().state, DeviceState::On);
}
