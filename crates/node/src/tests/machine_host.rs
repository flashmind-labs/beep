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

#[test]
fn a_wsl_machine_is_named_after_its_distribution_and_host() {
    assert_eq!(
        cut(&in_wsl("Ubuntu-24.04", "desk-pc.example.net")),
        "Ubuntu-24.04 on desk-pc"
    );
    assert_eq!(cut(&in_wsl("Ubuntu", "")), "Ubuntu");
}
