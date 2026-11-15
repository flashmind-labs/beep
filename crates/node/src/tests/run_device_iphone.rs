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
