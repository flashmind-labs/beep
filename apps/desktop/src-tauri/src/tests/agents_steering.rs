use super::*;

fn pasted(data: &[u8]) -> PastedImage {
    PastedImage {
        name: "shot.png".into(),
        mime: "image/png".into(),
        data: STANDARD.encode(data),
    }
}

#[test]
fn a_message_without_images_goes_as_text() {
    let steer = Steer::Prompt {
        text: "hi".into(),
        images: Vec::new(),
    };
    assert_eq!(
        AgentSteer::try_from(steer).unwrap(),
        AgentSteer::Prompt { text: "hi".into() }
    );
}

#[test]
fn images_go_as_prompt_images_within_the_limits() {
    let png = b"\x89PNG\r\n\x1a\n".to_vec();
    let steer = Steer::Prompt {
        text: "look".into(),
        images: vec![pasted(&png)],
    };
    let AgentSteer::PromptImages { images, .. } = AgentSteer::try_from(steer).unwrap() else {
        panic!("an image goes as PromptImages");
    };
    assert_eq!(images[0].data, png);
    let six = Steer::Prompt {
        text: "look".into(),
        images: (0..6).map(|_| pasted(&png)).collect(),
    };
    assert!(AgentSteer::try_from(six).is_err());
}

#[test]
fn a_steer_carries_its_images() {
    let png = b"\x89PNG\r\n\x1a\n".to_vec();
    let steer = AgentSteer::try_from(Steer::InTurn {
        text: "use the fixture".into(),
        images: vec![pasted(&png)],
    })
    .unwrap();
    let AgentSteer::Steer { images, .. } = &steer else {
        panic!("a steer goes as Steer");
    };
    assert_eq!(images[0].data, png);
}

#[test]
fn a_steer_that_may_have_gone_is_told_from_one_that_did_not() {
    let kind = |e: DesktopError| SteerError::from(e);
    assert!(matches!(
        kind(DesktopError::Offline("box".into())),
        SteerError::Lost { .. }
    ));
    assert!(matches!(
        kind(DesktopError::Lost("box".into())),
        SteerError::Lost { .. }
    ));
    assert!(matches!(
        kind(DesktopError::Unsure("box".into())),
        SteerError::Unsure { .. }
    ));
    assert!(matches!(
        kind(DesktopError::BadRequest("no".into())),
        SteerError::Failed { .. }
    ));
}

#[test]
fn stop_all_comes_from_the_page_as_stop_all() {
    let steer: Steer = serde_json::from_value(serde_json::json!("stop_all")).unwrap();
    assert_eq!(AgentSteer::try_from(steer).unwrap(), AgentSteer::StopAll {});
}

#[test]
fn new_requests_go_only_to_nodes_that_read_them() {
    let stops_all = |version| newer_than(version, &BEFORE_STOP_ALL);
    assert!(!stops_all(None));
    assert!(!stops_all(Some("")));
    assert!(!stops_all(Some("0.4.7")));
    assert!(!stops_all(Some("0.5.0")));
    assert!(stops_all(Some("0.5.1")));
    assert!(stops_all(Some("0.6.0")));
    assert!(!newer_than(Some("0.6.28"), &BEFORE_SET_DEVICE));
    assert!(newer_than(Some("0.6.29"), &BEFORE_SET_DEVICE));
}

#[test]
fn a_device_comes_from_the_page_as_set_device() {
    let machine = iroh::SecretKey::generate().public();
    let steer: Steer = serde_json::from_value(serde_json::json!({
        "device": { "device": { "machine": machine.to_string(), "device": "Pixel_9" } }
    }))
    .unwrap();
    let device = Some(SessionDevice {
        machine,
        device: DeviceId("Pixel_9".into()),
    });
    assert_eq!(
        AgentSteer::try_from(steer).unwrap(),
        AgentSteer::SetDevice { device }
    );

    let forget: Steer =
        serde_json::from_value(serde_json::json!({ "device": { "device": null } })).unwrap();
    assert_eq!(
        AgentSteer::try_from(forget).unwrap(),
        AgentSteer::SetDevice { device: None }
    );
}
