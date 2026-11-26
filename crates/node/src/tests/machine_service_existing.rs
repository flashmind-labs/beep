use super::{super::types::dir_arg, *};

fn from(dir: &str) -> Installed {
    Installed::From(PathBuf::from(dir))
}

#[test]
fn no_service_lets_a_join_install_one() {
    let action = decide(&Installed::No, Path::new("/srv/new")).unwrap();
    assert_eq!(action, Action::Install);
}

#[test]
fn a_service_from_this_folder_is_started_not_rewritten() {
    let action = decide(&from("/srv/node"), Path::new("/srv/node")).unwrap();
    assert_eq!(action, Action::Start);
}

#[test]
fn a_service_from_another_folder_is_left_alone() {
    let e = decide(&from("/srv/node"), Path::new("/srv/other")).unwrap_err();
    let e = e.to_string();
    assert!(
        e.starts_with(
            "A beep-node service already runs from /srv/node. Join with: beep-node --dir \
             /srv/node mesh join <ticket>"
        ),
        "{e}"
    );
    assert!(
        e.contains("`beep-node --dir /srv/other run` in another terminal"),
        "{e}"
    );
}

#[test]
fn a_service_whose_folder_is_unknown_is_left_alone() {
    let e = decide(&Installed::Unreadable, Path::new("/srv/other")).unwrap_err();
    assert!(e.to_string().contains("already installed"), "{e}");
}

#[test]
fn dir_arg_reads_the_folder_after_the_flag() {
    let args: Vec<String> = ["/bin/beep-node", "--dir", "/a b", "run"]
        .map(String::from)
        .to_vec();
    assert_eq!(dir_arg(&args), Some(PathBuf::from("/a b")));
    assert_eq!(dir_arg(&args[..2]), None);
}
