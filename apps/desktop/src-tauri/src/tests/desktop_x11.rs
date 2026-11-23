use super::Desktop;

#[test]
fn tracks_a_named_desktop_when_reordered_and_falls_back_after_deletion() {
    let desktop = Desktop {
        index: 2,
        name: Some("Work".into()),
    };
    assert_eq!(desktop.destination(2, &["Main".into(), "Work".into()]), 1);
    assert_eq!(
        desktop.destination(3, &["Main".into(), "Two".into(), "Three".into()]),
        0
    );
    assert_eq!(desktop.destination(2, &[]), 0);
}
