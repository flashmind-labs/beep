use super::*;

#[tokio::test(start_paused = true)]
async fn adb_installed_after_the_node_started_is_found_on_the_next_look() {
    let looks = std::cell::Cell::new(0);
    let started = tokio::time::Instant::now();
    let path = found(|| {
        looks.set(looks.get() + 1);
        (looks.get() == 3).then(|| PathBuf::from("/opt/sdk/platform-tools/adb"))
    })
    .await;

    assert_eq!(path, PathBuf::from("/opt/sdk/platform-tools/adb"));
    assert_eq!(looks.get(), 3);
    assert_eq!(started.elapsed(), TRACK_RETRY * 2);
}

#[tokio::test]
async fn a_list_while_adb_is_followed_is_what_the_follower_saw() {
    let phone = Device {
        id: DeviceId("R58M".to_owned()),
        name: "Pixel 9".to_owned(),
        platform: Platform::Android,
        state: DeviceState::On,
        os: None,
        phone: true,
    };
    PHONES.send_replace(vec![phone.clone()]);
    TRACKING.store(true, Ordering::Release);

    // No adb is asked: there is none here to say it.
    assert_eq!(list().await, vec![phone]);

    TRACKING.store(false, Ordering::Release);
    PHONES.send_replace(Vec::new());
}

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
    assert_eq!(
        gesture((100.4, 200.0), (104.0, 203.0), short),
        "input tap 100 200"
    );
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

#[test]
fn a_foldable_shows_the_screen_that_is_on() {
    let dumped = "  DisplayDeviceInfo{...}\n\
        mUniqueId=local:111\n\
        mPhysicalDisplayId=111\n\
        mState=OFF\n\
        mUniqueId=local:222\n\
        mPhysicalDisplayId=222\n\
        mState=ON\n\
      mState=AUTO_BRIGHTNESS_ENABLED\n";
    assert_eq!(lit_screen(dumped).as_deref(), Some("222"));
    assert_eq!(
        lit_screen("    mPhysicalDisplayId=111\n    mState=ON\n"),
        None
    );
}

#[test]
fn a_warning_before_the_screenshot_is_skipped() {
    let mut png = Vec::new();
    image::RgbImage::new(4, 8)
        .write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png)
        .expect("encoded");
    let mut printed = b"[Warning] Multiple displays were found\n".to_vec();
    printed.extend(png);
    let (full, frame) = frame(&printed, 800).expect("a frame");
    assert_eq!(full, (4, 8));
    assert_eq!((frame.width, frame.height), (4, 8));
}
