use super::*;

#[test]
fn the_checksum_refuses_another_file() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let file = dir.join("teacli");
    fs::write(&file, b"hello\n").unwrap();
    // `printf 'hello\n' | sha256sum`
    let right = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
    verify(&file, right).unwrap();
    verify(&file, &right.to_uppercase()).unwrap();
    fs::write(&file, b"hello, tampered\n").unwrap();
    let err = verify(&file, right).unwrap_err();
    assert!(format!("{err:#}").contains("not the expected"), "{err:#}");
}

#[test]
fn a_release_is_checked_against_its_own_line() {
    let sums = "aaa  gh_2.103.3_linux_amd64.tar.gz\nbbb  gh_2.103.3_linux_arm64.tar.gz\n";
    let gh = release(Tool::Gh, "2.103.3", "linux", "aarch64").unwrap();
    assert_eq!(
        gh.url,
        "https://github.com/cli/cli/releases/download/v2.103.3/gh_2.103.3_linux_arm64.tar.gz"
    );
    assert_eq!(
        listed_sha256(sums, "gh_2.103.3_linux_arm64.tar.gz"),
        Some("bbb")
    );
    assert_eq!(listed_sha256(sums, "gh_2.103.3_macOS_arm64.zip"), None);
    let teacli = release(Tool::Teacli, "1.0.8", "macos", "x86_64").unwrap();
    assert!(
        teacli.url.ends_with("/v1.0.8/teacli-darwin-amd64"),
        "{}",
        teacli.url
    );
    let teacli = release(Tool::Teacli, "1.0.8", "windows", "x86_64").unwrap();
    assert!(
        teacli.url.ends_with("/v1.0.8/teacli-windows-amd64.exe"),
        "{}",
        teacli.url
    );
    let flsh = release(Tool::Flsh, "0.2.0", "macos", "aarch64").unwrap();
    assert_eq!(
        flsh.url,
        "https://releases.flashmind.com/flashmind/cli/0.2.0/flsh-aarch64-apple-darwin"
    );
}

#[test]
fn flsh_releases_use_the_platform_binary_and_checksum() {
    for (os, arch, target, suffix) in [
        ("linux", "x86_64", "x86_64-unknown-linux-gnu", ""),
        ("linux", "aarch64", "aarch64-unknown-linux-gnu", ""),
        ("macos", "x86_64", "x86_64-apple-darwin", ""),
        ("macos", "aarch64", "aarch64-apple-darwin", ""),
        ("windows", "x86_64", "x86_64-pc-windows-msvc", ".exe"),
    ] {
        assert_eq!(flsh_target(os, arch).as_deref(), Some(target));

        let flsh = release(Tool::Flsh, "0.3.2", os, arch).unwrap();
        let url =
            format!("https://releases.flashmind.com/flashmind/cli/0.3.2/flsh-{target}{suffix}");
        assert_eq!(flsh.name, "flsh");
        assert_eq!(flsh.url, url);
        assert!(flsh.member.is_none());

        let Sha256Of::Alone(checksum) = flsh.sha256 else {
            panic!("flsh must use its binary's checksum file");
        };
        assert_eq!(checksum, format!("{url}.sha256"));
    }
}

#[test]
fn flsh_rejects_unsupported_platforms() {
    for (os, arch) in [
        ("windows", "aarch64"),
        ("windows", "x86"),
        ("linux", "x86"),
        ("freebsd", "x86_64"),
    ] {
        assert_eq!(flsh_target(os, arch), None);

        let error = release(Tool::Flsh, "0.3.2", os, arch).err().unwrap();
        assert_eq!(
            error.to_string(),
            format!("no flsh release for {os} {arch}")
        );
    }
}

/// A gzipped tarball like Node.js's: an executable, a symlink to a file
/// beside it, and an entry that tries to climb out.
#[cfg(unix)]
fn node_like_tarball(path: &Path) {
    let mut tarball = tar::Builder::new(flate2::write::GzEncoder::new(
        fs::File::create(path).expect("created"),
        flate2::Compression::fast(),
    ));
    let mut file = |name: &str, mode: u32, body: &[u8]| {
        let mut header = tar::Header::new_gnu();
        header.set_size(u64::try_from(body.len()).expect("small"));
        header.set_mode(mode);
        header.set_entry_type(tar::EntryType::Regular);
        tarball
            .append_data(&mut header, name, body)
            .expect("appended");
    };
    file("node-v1/bin/node", 0o755, b"#!/bin/sh\n");
    file("node-v1/lib/npm-cli.js", 0o644, b"// npm\n");
    let mut link = tar::Header::new_gnu();
    link.set_entry_type(tar::EntryType::Symlink);
    link.set_size(0);
    link.set_mode(0o777);
    tarball
        .append_link(&mut link, "node-v1/bin/npm", "../lib/npm-cli.js")
        .expect("linked");
    let mut climb = tar::Header::new_old();
    climb.as_old_mut().name[..9].copy_from_slice(b"../escape");
    climb.set_size(1);
    climb.set_mode(0o644);
    climb.set_entry_type(tar::EntryType::Regular);
    climb.set_cksum();
    tarball.append(&climb, &b"x"[..]).expect("appended");
    tarball
        .into_inner()
        .expect("written")
        .finish()
        .expect("finished");
}

#[cfg(unix)]
#[tokio::test]
async fn a_tarball_unpacks_in_process_with_links_and_modes_kept_inside_its_folder() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("a folder");
    let archive = dir.path().join("node-v1.tar.gz");
    let into = dir.path().join("stage");
    fs::create_dir_all(&into).expect("stage");
    node_like_tarball(&archive);

    unpack(&archive, &into).await.expect("unpacked");

    let node = into.join("node-v1/bin/node");
    let mode = fs::metadata(&node).expect("node").permissions().mode();
    assert_eq!(mode & 0o111, 0o111, "node stays executable");
    let npm = into.join("node-v1/bin/npm");
    assert_eq!(
        fs::read_link(&npm).expect("a symlink"),
        Path::new("../lib/npm-cli.js")
    );
    assert_eq!(fs::read(&npm).expect("followed"), b"// npm\n");
    assert!(!dir.path().join("escape").exists(), "nothing lands outside");
}

#[tokio::test]
async fn a_zip_unpacks_in_process() {
    use std::io::Write;

    let dir = tempfile::tempdir().expect("a folder");
    let archive = dir.path().join("node-v1.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive).expect("created"));
    zip.start_file("node-v1/node.exe", zip::write::SimpleFileOptions::default())
        .expect("started");
    zip.write_all(b"MZ").expect("written");
    zip.finish().expect("finished");

    unpack(&archive, dir.path()).await.expect("unpacked");

    assert_eq!(
        fs::read(dir.path().join("node-v1/node.exe")).expect("read"),
        b"MZ"
    );
}

#[test]
fn flsh_lives_in_the_users_own_bin_folder() {
    let home = Path::new("/home/someone");
    assert_eq!(flsh_bin(home), home.join(".local").join("bin"));
}

#[test]
fn a_flsh_in_the_nodes_folder_moves_to_the_users_own() {
    let temp = tempfile::tempdir().unwrap();
    let (node, own) = (
        temp.path().join("node/bin"),
        temp.path().join("home/.local/bin"),
    );
    let name = format!("flsh{}", env::consts::EXE_SUFFIX);
    fs::create_dir_all(&node).unwrap();
    fs::write(node.join(&name), b"old").unwrap();

    settle_flsh(&node, &own).unwrap();

    assert!(!node.join(&name).exists(), "the node's copy is gone");
    assert_eq!(fs::read(own.join(&name)).unwrap(), b"old");
}

#[test]
fn the_users_own_flsh_wins_over_the_nodes_copy() {
    let temp = tempfile::tempdir().unwrap();
    let (node, own) = (
        temp.path().join("node/bin"),
        temp.path().join("home/.local/bin"),
    );
    let name = format!("flsh{}", env::consts::EXE_SUFFIX);
    fs::create_dir_all(&node).unwrap();
    fs::create_dir_all(&own).unwrap();
    fs::write(node.join(&name), b"old").unwrap();
    fs::write(own.join(&name), b"new").unwrap();

    settle_flsh(&node, &own).unwrap();

    assert!(!node.join(&name).exists(), "the node's copy is gone");
    assert_eq!(fs::read(own.join(&name)).unwrap(), b"new");
}

#[test]
fn settling_with_no_flsh_anywhere_does_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let (node, own) = (
        temp.path().join("node/bin"),
        temp.path().join("home/.local/bin"),
    );

    settle_flsh(&node, &own).unwrap();

    assert!(!own.exists());
}
