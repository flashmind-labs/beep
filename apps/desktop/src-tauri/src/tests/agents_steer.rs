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
fn a_steer_carries_its_images_and_falls_back_to_a_prompt() {
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
    assert!(matches!(
        queued_instead(&steer),
        Some(AgentSteer::PromptImages { text, .. }) if text == "use the fixture"
    ));
    let old = DesktopError::Api(ApiError::bad_request("unknown op 5"));
    assert!(older_than(&old, &[5, 6]));
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
fn an_old_server_is_told_apart() {
    let old = DesktopError::Api(ApiError::bad_request("unknown op 3"));
    assert!(older_than(&old, &[3]));
    assert!(!older_than(
        &DesktopError::Api(ApiError::bad_request("The message is empty.")),
        &[3]
    ));
}
