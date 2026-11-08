use super::*;

#[test]
fn the_binary_is_replaced_in_place() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let exe = dir.path().join(if cfg!(windows) {
        "beep-node.exe"
    } else {
        "beep-node"
    });
    fs::write(&exe, b"old").expect("writing the old binary");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o750)).expect("chmod");
    }
    writable(&exe).expect("the folder is writable");
    replace(&exe, b"new").expect("replaced");
    assert_eq!(fs::read(&exe).expect("reading it back"), b"new");
    assert!(!staged(&exe).exists(), "the staged copy was renamed");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&exe).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o750);
    }
}

#[test]
fn the_old_binary_moves_aside_next_to_the_new_one() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let exe = dir.path().join("beep-node.exe");
    let fresh = staged(&exe);
    let old = aside(&exe);
    assert_eq!(old, dir.path().join("beep-node.exe.old"));
    fs::write(&exe, b"running").expect("writing the old binary");
    fs::write(&fresh, b"new").expect("writing the new binary");
    swap_aside(&exe, &fresh, &old).expect("swapped");
    assert_eq!(fs::read(&exe).expect("reading exe"), b"new");
    assert_eq!(fs::read(&old).expect("reading old"), b"running");
    assert!(!fresh.exists(), "the staged copy was renamed");
}

#[test]
fn the_copy_an_earlier_update_left_is_replaced() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let exe = dir.path().join("beep-node.exe");
    let fresh = staged(&exe);
    let old = aside(&exe);
    fs::write(&old, b"two updates ago").expect("writing the leftover");
    fs::write(&exe, b"running").expect("writing the old binary");
    fs::write(&fresh, b"new").expect("writing the new binary");
    swap_aside(&exe, &fresh, &old).expect("swapped");
    assert_eq!(fs::read(&exe).expect("reading exe"), b"new");
    assert_eq!(fs::read(&old).expect("reading old"), b"running");
}

#[test]
fn a_failed_swap_puts_the_old_binary_back() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let exe = dir.path().join("beep-node.exe");
    let old = aside(&exe);
    fs::write(&exe, b"running").expect("writing the old binary");
    // No staged copy, so the second rename fails.
    let missing = staged(&exe);
    assert!(swap_aside(&exe, &missing, &old).is_err());
    assert_eq!(fs::read(&exe).expect("reading exe"), b"running");
    assert!(!old.exists(), "the old binary went back");
}
