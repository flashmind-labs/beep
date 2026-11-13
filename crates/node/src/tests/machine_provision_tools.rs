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
        let url = format!(
            "https://releases.flashmind.com/flashmind/cli/0.3.2/flsh-{target}{suffix}"
        );
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
        assert_eq!(error.to_string(), format!("no flsh release for {os} {arch}"));
    }
}
