use super::*;
use uuid::Uuid;

fn path() -> PathBuf {
    std::env::temp_dir()
        .join(format!("beep-home-frame-{}", Uuid::new_v4()))
        .join(FILE)
}

#[test]
fn a_frame_loads_back_only_under_its_own_version() {
    let path = path();
    let frame = HomeFrame {
        skin: Some("linear".into()),
        html: "<div class=\"cwin kept\"></div>".into(),
    };
    assert_eq!(load_from(&path, "1.0.0"), None);
    save_to(&path, "1.0.0", &frame).unwrap();

    assert_eq!(load_from(&path, "1.0.0"), Some(frame));
    assert_eq!(load_from(&path, "1.0.1"), None);
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn the_script_carries_any_html_as_one_string_literal() {
    let html = "a \" b \\ c \n d </script> e \u{2028} f";
    let frame = HomeFrame {
        skin: None,
        html: html.into(),
    };
    let script = script(&frame);
    let literal = script
        .lines()
        .find_map(|line| line.strip_prefix("const frame = "))
        .and_then(|line| line.strip_suffix(';'))
        .unwrap();
    let parsed: HomeFrame = serde_json::from_str(literal).unwrap();

    assert_eq!(parsed, frame);
}
