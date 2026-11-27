use super::*;

const LIST: &str = r#"{
  "info" : { "outcome" : "success" },
  "result" : {
    "devices" : [
      {
        "connectionProperties" : { "pairingState" : "paired", "tunnelState" : "connected", "transportType" : "wired" },
        "deviceProperties" : { "name" : "Test Phone", "osVersionNumber" : "27.0" },
        "hardwareProperties" : { "deviceType" : "iPhone", "reality" : "physical", "udid" : "00000000-0000000000000001" }
      },
      {
        "connectionProperties" : { "pairingState" : "paired", "tunnelState" : "unavailable" },
        "deviceProperties" : { "name" : "Away Pad" },
        "hardwareProperties" : { "deviceType" : "iPad", "reality" : "physical", "udid" : "00000000-0000000000000002" }
      },
      {
        "connectionProperties" : { "pairingState" : "paired", "tunnelState" : "disconnected" },
        "deviceProperties" : { "name" : "Watch" },
        "hardwareProperties" : { "deviceType" : "appleWatch", "reality" : "physical", "udid" : "00000000-0000000000000003" }
      },
      {
        "connectionProperties" : { "pairingState" : "paired", "tunnelState" : "disconnected" },
        "deviceProperties" : { "name" : "iPhone 17", "osVersionNumber" : "27.2" },
        "hardwareProperties" : { "deviceType" : "iPhone", "reality" : "simulated", "udid" : "00000000-0000-0000-0000-000000000004" }
      }
    ]
  }
}"#;

#[test]
fn lists_reachable_iphones_and_ipads_only() {
    let devices = parse_devices(LIST).expect("the list reads");

    assert_eq!(
        devices,
        vec![Device {
            id: DeviceId("00000000-0000000000000001".to_owned()),
            name: "Test Phone".to_owned(),
            platform: Platform::Ios,
            state: DeviceState::On,
            os: Some("iOS 27.0".to_owned()),
            phone: true,
        }]
    );
}

#[tokio::test]
#[ignore = "needs an iPhone plugged into this Mac"]
async fn shows_a_plugged_in_iphone() {
    let iphone = list()
        .await
        .into_iter()
        .next()
        .expect("an iPhone is plugged in");
    let mut live = attach(&iphone.id, 512).await.expect("the iPhone attaches");

    let shown = tokio::time::timeout(CALL_TIMEOUT, async {
        loop {
            live.screen.changed().await.expect("the view runs");
            let screen = beep_proto::watched::read_and_update(&mut live.screen, Clone::clone);

            match screen {
                Screen::Starting => {}
                Screen::Frame(frame) => break frame,
                Screen::Ended(reason) => panic!("{reason}"),
            }
        }
    })
    .await
    .expect("a frame comes in time");

    assert!(shown.width.max(shown.height) <= 512);
    assert!(!shown.jpeg.is_empty());
}
