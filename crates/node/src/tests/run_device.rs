use super::*;
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

    if booted {
        let reply = ask(Request::Shutdown { id }).await;
        assert_eq!(reply, Reply::Done {});
    }
}
