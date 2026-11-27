use super::*;

const LIST: &str = r#"{
  "devices" : {
    "com.apple.CoreSimulator.SimRuntime.watchOS-26-5" : [
      {
        "udid" : "00000000-0000-0000-0000-000000000001",
        "isAvailable" : true,
        "deviceTypeIdentifier" : "com.apple.CoreSimulator.SimDeviceType.Apple-Watch-Series-11-46mm",
        "state" : "Shutdown",
        "name" : "Apple Watch Series 11 (46mm)"
      }
    ],
    "com.apple.CoreSimulator.SimRuntime.iOS-26-5" : [
      {
        "dataPath" : "\/path\/to\/data",
        "udid" : "00000000-0000-0000-0000-000000000002",
        "isAvailable" : true,
        "deviceTypeIdentifier" : "com.apple.CoreSimulator.SimDeviceType.iPhone-17",
        "state" : "Booted",
        "name" : "iPhone 17"
      },
      {
        "udid" : "00000000-0000-0000-0000-000000000003",
        "isAvailable" : true,
        "deviceTypeIdentifier" : "com.apple.CoreSimulator.SimDeviceType.iPad-Air-11-inch-M3",
        "state" : "Shutdown",
        "name" : "iPad Air 11-inch (M3)"
      },
      {
        "udid" : "00000000-0000-0000-0000-000000000004",
        "isAvailable" : false,
        "deviceTypeIdentifier" : "com.apple.CoreSimulator.SimDeviceType.iPhone-16",
        "state" : "Shutdown",
        "name" : "iPhone 16"
      }
    ]
  }
}"#;

#[test]
fn simctl_lists_the_available_iphones_and_ipads_with_their_ios() {
    let devices = parse_simulators(LIST).expect("parsed");
    let summary: Vec<_> = devices
        .iter()
        .map(|device| (device.name.as_str(), device.state, device.os.as_deref()))
        .collect();
    assert_eq!(
        summary,
        [
            ("iPhone 17", DeviceState::On, Some("iOS 26.5")),
            ("iPad Air 11-inch (M3)", DeviceState::Off, Some("iOS 26.5")),
        ]
    );
    assert_eq!(devices[0].id.0, "00000000-0000-0000-0000-000000000002");
    assert!(
        devices
            .iter()
            .all(|device| device.platform == Platform::Ios)
    );
}

#[test]
fn text_types_as_us_keys_with_shift_for_capitals_and_symbols() {
    assert_eq!(us_key('a'), Some((4, false)));
    assert_eq!(us_key('Z'), Some((29, true)));
    assert_eq!(us_key('0'), Some((39, false)));
    assert_eq!(us_key('!'), Some((30, true)));
    assert_eq!(us_key('?'), Some((56, true)));
    assert_eq!(us_key('/'), Some((56, false)));
    assert_eq!(us_key('é'), None);
    // Down and up for each key, shift around the capital.
    assert_eq!(typed("aB").len(), 2 + 4);
}
