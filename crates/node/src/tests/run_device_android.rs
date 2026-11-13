use super::*;

#[test]
fn a_discovery_file_names_the_avd_and_its_grpc_service() {
    let text = "emulator.build=14788078\n\
        avd.id=Pixel_9_Pro\n\
        port.serial=5554\n\
        avd.name=Pixel_9_Pro\n\
        avd.dir=/path/to/avd/Pixel_9_Pro.avd\n\
        cmdline=\"/path/to/qemu\" \"-avd\" \"Pixel_9_Pro\" \"-no-window\"\n\
        grpc.token=c2VjcmV0+dG9rZW4=\n\
        grpc.port=8554\n";
    let running = parse_discovery(text).expect("parsed");
    assert_eq!(running.avd, "Pixel_9_Pro");
    assert_eq!(running.port, 8554);
    assert_eq!(running.token.expose_secret(), "c2VjcmV0+dG9rZW4=");
    assert_eq!(
        running.dir,
        Some(PathBuf::from("/path/to/avd/Pixel_9_Pro.avd"))
    );
}

#[test]
fn an_emulator_without_a_grpc_token_is_not_reached() {
    assert!(parse_discovery("avd.name=Pixel_7\ngrpc.port=8554\n").is_none());
}

#[test]
fn an_avd_config_gives_its_name_and_android_release() {
    let text = "avd.ini.displayname=Pixel 9 Pro\nhw.lcd.width=1280\ntarget=android-36\n";
    let config = AvdConfig::parse(text).expect("parsed");
    assert_eq!(config.name.as_deref(), Some("Pixel 9 Pro"));
    assert_eq!(config.os.as_deref(), Some("Android 16"));

    let unnamed = device(
        "Pixel_7".to_owned(),
        DeviceState::Off,
        AvdConfig::parse("target=android-34\n"),
    );
    assert_eq!(unnamed.name, "Pixel 7");
    assert_eq!(unnamed.os.as_deref(), Some("Android 14"));
}

#[test]
fn api_levels_read_as_android_releases() {
    assert_eq!(android_version("36.1").as_deref(), Some("Android 16"));
    assert_eq!(android_version("32").as_deref(), Some("Android 12L"));
    assert_eq!(android_version("28").as_deref(), Some("API 28"));
    assert_eq!(android_version("CANARY"), None);
}

#[test]
fn rotating_goes_round_a_quarter_turn_at_a_time() {
    assert_eq!(quarter_turn(0.0), 90.0);
    assert_eq!(quarter_turn(90.0), 180.0);
    assert_eq!(quarter_turn(180.0), -90.0);
    assert_eq!(quarter_turn(-90.0), 0.0);
}

#[test]
fn a_touch_lands_on_a_pixel_of_the_screen() {
    assert_eq!(pixel(1280.0, 1280), 1279);
    assert_eq!(pixel(-3.0, 1280), 0);
    assert_eq!(pixel(639.6, 1280), 640);
}
