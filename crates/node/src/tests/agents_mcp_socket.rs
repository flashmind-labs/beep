use super::*;

#[test]
fn a_long_home_gets_a_short_socket_of_its_own() {
    let user = "a".repeat(64);
    let dir = PathBuf::from(format!(
        "/Users/{user}/Library/Application Support/com.flashmind.beep.desktop/node"
    ));
    // macOS's per-user TMPDIR.
    let tmp = Path::new("/var/folders/zz/zyxvpxvq6csfxvn_n0000000000000/T/");
    let socket = choose(&dir, Some(tmp), 501);
    assert!(socket.as_os_str().len() < 104, "{}", socket.display());
    assert!(
        socket.starts_with(tmp.join("beep-501")),
        "{}",
        socket.display()
    );
    let other = dir.with_file_name("other");
    assert_ne!(socket, choose(&other, Some(tmp), 501), "one per node");
    // A TMPDIR as long as the home falls back to /tmp.
    let long_tmp = dir.join("tmp");
    let socket = choose(&dir, Some(&long_tmp), 501);
    assert!(socket.starts_with("/tmp/beep-501"), "{}", socket.display());
    assert!(socket.as_os_str().len() < 104);
    assert!(choose(&dir, None, 501).starts_with("/tmp/beep-501"));
}

#[test]
fn the_short_folder_is_private() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let base = tempfile::tempdir().expect("a temporary folder");
    let socket = base.path().join("beep-1").join("mcp-0.sock");
    prepare(&socket, Path::new("/srv/node")).expect("made");
    let meta = std::fs::metadata(base.path().join("beep-1")).expect("there");
    assert_eq!(meta.mode() & 0o777, 0o700);
    std::fs::set_permissions(
        base.path().join("beep-1"),
        std::fs::Permissions::from_mode(0o755),
    )
    .expect("opened up");
    assert!(prepare(&socket, Path::new("/srv/node")).is_err());
}
