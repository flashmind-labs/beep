use super::*;

#[test]
fn tray_items_name_their_view() {
    assert_eq!(tray_target("settings"), Some(CenterTarget::Settings));
    assert_eq!(tray_target("check-update"), None);
    assert_eq!(tray_target(waiting::MORE), Some(CenterTarget::Home));
    assert_eq!(tray_target("report-problem"), Some(CenterTarget::Report));
}
