use super::*;

#[test]
fn adb_lists_ready_phones_by_model_without_emulators() {
    let text = "List of devices attached\n\
        R5CT1234ABC            device usb:1-1 product:husky model:Pixel_8_Pro device:husky transport_id:3\n\
        192.168.1.20:5555      device product:x model:SM_S928B device:e3q transport_id:4\n\
        emulator-5554          device product:sdk_gphone64 model:sdk_gphone64_arm64 transport_id:1\n\
        0A1B2C3D               unauthorized usb:1-2 transport_id:5\n\
        * daemon started successfully\n";
    let found = attached(text);
    assert_eq!(
        found,
        [
            Attached {
                id: DeviceId("R5CT1234ABC".to_owned()),
                name: "Pixel 8 Pro".to_owned(),
            },
            Attached {
                id: DeviceId("192.168.1.20:5555".to_owned()),
                name: "SM S928B".to_owned(),
            },
        ]
    );
}

#[test]
fn a_gesture_is_a_tap_a_long_press_or_a_swipe() {
    let short = Duration::from_millis(60);
    assert_eq!(gesture((100.4, 200.0), (104.0, 203.0), short), "input tap 100 200");
    assert_eq!(
        gesture((100.0, 200.0), (104.0, 203.0), Duration::from_millis(800)),
        "input swipe 100 200 100 200 800"
    );
    assert_eq!(
        gesture((100.0, 900.0), (100.0, 300.0), Duration::from_millis(250)),
        "input swipe 100 900 100 300 250"
    );
}

#[test]
fn typed_text_is_quoted_with_enter_between_lines() {
    assert_eq!(typed("it's here"), r"input text 'it'\''s%shere'");
    assert_eq!(
        typed("a\n\nb"),
        "input text 'a'; input keyevent KEYCODE_ENTER; input keyevent KEYCODE_ENTER; input text 'b'"
    );
    assert_eq!(typed("\n"), "input keyevent KEYCODE_ENTER");
}
