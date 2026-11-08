use super::*;

#[test]
fn only_the_launchd_path_counts_as_bare() {
    assert!(bare("/usr/bin:/bin:/usr/sbin:/sbin"));
    assert!(bare(""));
    assert!(!bare("/opt/homebrew/bin:/usr/bin:/bin"));
}

#[test]
fn the_path_is_read_past_what_rc_files_print() {
    let out = format!("welcome back\n{MARK}/opt/homebrew/bin:/usr/bin{MARK}");
    assert_eq!(parse(&out).as_deref(), Some("/opt/homebrew/bin:/usr/bin"));
    assert_eq!(parse("no marks here"), None);
    assert_eq!(parse(&format!("{MARK}{MARK}")), None);
}

#[test]
fn a_login_shell_gives_its_path() {
    let path = login_path("/bin/sh").expect("sh prints its PATH");
    assert!(!path.is_empty());
}
