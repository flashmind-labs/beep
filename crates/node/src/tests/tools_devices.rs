use super::*;
use beep_proto::node::device::DeviceId;
use serde_json::json;

fn device(id: &str, name: &str) -> Device {
    Device {
        id: DeviceId(id.to_owned()),
        name: name.to_owned(),
        platform: Platform::Ios,
        state: DeviceState::On,
        os: Some("iOS 26.5".to_owned()),
        phone: false,
    }
}

fn frame(width: u32, height: u32) -> Frame {
    Frame {
        jpeg: vec![0xff, 0xd8],
        width,
        height,
    }
}

#[test]
fn calls_read_into_typed_actions_and_name_what_is_missing() {
    let tap = Action::try_from(json!({
        "action": "tap", "machine": "mac-box", "device": "iPhone 17", "x": 120, "y": 400.5
    }));
    assert_eq!(
        tap,
        Ok(Action::Tap {
            at: Target {
                machine: "mac-box".to_owned(),
                device: "iPhone 17".to_owned()
            },
            x: 120.0,
            y: 400.5
        })
    );
    assert_eq!(
        Action::try_from(json!({ "action": "list" })),
        Ok(Action::List {})
    );

    let press = Action::try_from(json!({
        "action": "press", "machine": "m", "device": "d", "key": "app_switch"
    }));
    assert!(matches!(
        press,
        Ok(Action::Press {
            key: Press::AppSwitch,
            ..
        })
    ));

    for (args, missing) in [
        (
            json!({ "action": "tap", "machine": "m", "device": "d", "x": 1 }),
            "`y`",
        ),
        (json!({ "action": "boot", "machine": "m" }), "`device`"),
        (json!({ "machine": "m", "device": "d" }), "`action`"),
        (json!({ "action": "fly", "machine": "m" }), "`fly`"),
        (
            json!({ "action": "press", "machine": "m", "device": "d", "key": "f5" }),
            "`f5`",
        ),
    ] {
        let Err(e) = Action::try_from(args.clone()) else {
            panic!("{args} read");
        };
        assert!(e.contains(missing), "{e}");
    }
}

#[test]
fn screenshot_pixels_become_fractions_of_the_screen() {
    let shot = frame(590, 1280);
    assert_eq!(fraction(295.0, 640.0, &shot).unwrap(), (0.5, 0.5));
    assert_eq!(fraction(590.0, 0.0, &shot).unwrap(), (1.0, 0.0));

    for (x, y) in [(-1.0, 10.0), (10.0, 1281.0), (f64::NAN, 1.0)] {
        let e = fraction(x, y, &shot).unwrap_err().to_string();
        assert!(e.contains("590 x 1280"), "{e}");
    }
}

#[test]
fn a_swipe_goes_down_moves_evenly_and_comes_up_at_its_end() {
    let inputs = swipe((0.5, 0.8), (0.5, 0.2), Duration::from_millis(450));
    let len = usize::try_from(SWIPE_MOVES).unwrap() + 2;
    assert_eq!(inputs.len(), len);
    assert_eq!(inputs[0].0, touch(TouchPhase::Down, 0.5, 0.8));
    assert_eq!(
        inputs[len - 1],
        (touch(TouchPhase::Up, 0.5, 0.2), Duration::ZERO)
    );

    let total: Duration = inputs.iter().map(|(_, pause)| *pause).sum();
    assert_eq!(total, Duration::from_millis(450));

    let ys: Vec<f32> = inputs
        .iter()
        .map(|(input, _)| match input {
            Input::Touch { y, .. } => *y,
            other => panic!("{other:?}"),
        })
        .collect();
    assert!(ys.windows(2).all(|pair| pair[1] < pair[0]), "{ys:?}");
}

#[test]
fn a_device_is_found_by_id_or_name_and_a_shared_name_needs_the_id() {
    let devices = [
        device("A1", "iPhone 17"),
        device("B2", "iPad Air"),
        device("C3", "iPad Air"),
    ];
    assert_eq!(pick(&devices, "A1").unwrap().id.0, "A1");
    assert_eq!(pick(&devices, " iphone 17 ").unwrap().id.0, "A1");
    assert_eq!(pick(&devices, "C3").unwrap().id.0, "C3");

    let e = pick(&devices, "iPad Air").unwrap_err();
    assert!(e.contains("B2, C3"), "{e}");
    let e = pick(&devices, "Pixel 9").unwrap_err();
    assert!(
        e.contains("No device Pixel 9") && e.contains("iPhone 17 (A1)"),
        "{e}"
    );
    assert!(pick(&[], "Pixel 9").is_err());
}

#[test]
fn keys_and_buttons_become_device_input() {
    assert_eq!(Input::from(Press::Return), Input::Key { key: Key::Return });
    assert_eq!(
        Input::from(Press::VolumeDown),
        Input::Button {
            button: Button::VolumeDown
        }
    );
}

#[tokio::test(start_paused = true)]
async fn an_attached_device_answers_input_and_closes_once_idle() {
    let local = beep_proto::node::local_link();
    let (mut node_tx, mut node_rx) = local.node;
    let (tx, rx) = local.caller;
    let viewer = Viewer::start(
        Link {
            tx,
            rx,
            _here: None,
        },
        Duration::from_secs(30),
        true,
    );

    node_tx.send(&Reply::Frame(frame(100, 200))).await.unwrap();
    assert_eq!(viewer.shot().await.unwrap().height, 200);

    viewer
        .input(vec![(Press::Home.into(), Duration::ZERO)])
        .await
        .unwrap();
    let sent: Request = node_rx.recv().await.unwrap().unwrap();
    assert_eq!(
        sent,
        Request::Input {
            input: Input::Button {
                button: Button::Home
            }
        }
    );

    // Nothing more is asked: the view lets the stream go, and the device
    // side sees it end.
    let ended: Option<Request> = node_rx.recv().await.unwrap();
    assert_eq!(ended, None);
    assert!(viewer.asks.is_closed());
}

#[tokio::test]
async fn a_call_names_the_machines_it_could_not_find() {
    use iroh::{Endpoint, endpoint::presets};

    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let node = Node::test(dir.path()).await;
    let screens = Screens::default();

    let called = call(
        &node,
        &mesh,
        &screens,
        &SessionId::from("s1"),
        json!({ "action": "boot", "machine": "nowhere", "device": "Pixel 9" }),
    )
    .await;
    assert!(called.failed);
    assert_eq!(
        called.text,
        "No machine called nowhere. Machines: build-box."
    );
}

fn attached(machine: EndpointId, id: &str, called: &str) -> (Attached, mpsc::Receiver<Ask>) {
    let (asks, taken) = mpsc::channel(1);
    let task = tokio::spawn(async {});
    let viewer = Viewer {
        asks,
        takes_input: true,
        _task: std::sync::Arc::new(AbortOnDropHandle::new(task)),
    };
    let attached = Attached {
        machine,
        id: id.to_owned(),
        called: called.to_owned(),
        viewer,
    };
    (attached, taken)
}

fn machine_id() -> EndpointId {
    iroh::SecretKey::from_bytes(&[7; 32]).public()
}

#[tokio::test]
async fn a_fifth_device_detaches_the_least_recently_used_one() {
    let screens = Screens::default();
    let machine = machine_id();
    let mut keep = Vec::new();

    for n in 0..MAX_ATTACHED {
        let (a, rx) = attached(machine, &format!("id{n}"), &format!("d{n}"));
        keep.push(rx);
        screens.attach(a);
    }

    // d0 is used again, so d1 is now the least recently used.
    assert!(screens.touch(machine, "d0").is_some());
    let (a, rx) = attached(machine, "id9", "d9");
    keep.push(rx);
    screens.attach(a);

    assert!(screens.touch(machine, "d1").is_none());
    assert!(screens.touch(machine, "d0").is_some());
    assert!(screens.touch(machine, "d9").is_some());
}

#[tokio::test]
async fn shutting_down_by_id_or_name_detaches_the_device() {
    let screens = Screens::default();
    let machine = machine_id();
    let (a, _rx) = attached(machine, "emulator-5554", "Pixel 9");
    screens.attach(a);

    screens.close(machine, "emulator-5554", "emulator-5554");
    assert!(screens.touch(machine, "Pixel 9").is_none());

    let (a, _rx2) = attached(machine, "emulator-5554", "emulator-5554");
    screens.attach(a);
    screens.close(machine, "emulator-5554", "Pixel 9");
    assert!(screens.touch(machine, "emulator-5554").is_none());
}

#[tokio::test]
async fn a_device_attached_by_name_is_found_again_by_its_id() {
    let screens = Screens::default();
    let machine = machine_id();
    let (a, _rx) = attached(machine, "emulator-5554", "emulator-5554");
    screens.attach(a);

    // A call by name misses, and the lookup then resolves the id and finds it.
    assert!(screens.touch(machine, "Pixel 9").is_none());
    assert!(screens.touch(machine, "emulator-5554").is_some());

    let (a, _rx2) = attached(machine, "emulator-5554", "Pixel 9");
    screens.attach(a);
    assert_eq!(crate::lock(&screens.open).len(), 1);
}
