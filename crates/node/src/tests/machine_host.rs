use super::*;

#[test]
fn a_long_host_name_is_cut_not_replaced() {
    let long = "h".repeat(MAX_MACHINE_NAME + 10);
    assert_eq!(short_name(&long), "h".repeat(MAX_MACHINE_NAME));
}

#[test]
fn a_cut_keeps_whole_characters() {
    let long = "\u{e9}".repeat(MAX_MACHINE_NAME);
    let cut = short_name(&long);
    assert_eq!(cut, "\u{e9}".repeat(MAX_MACHINE_NAME / 2));
}

#[test]
fn the_first_label_is_the_name_and_empty_falls_back() {
    assert_eq!(short_name("build-box.example.net"), "build-box");
    assert_eq!(short_name(""), NO_HOSTNAME);
    assert_eq!(short_name(".example"), NO_HOSTNAME);
}
