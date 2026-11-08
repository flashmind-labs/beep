use super::*;

#[test]
fn each_machine_gets_its_own_build() {
    let linux = node_dist("linux", "x86_64", false).expect("a build");
    assert_eq!(
        linux.url("v24.11.1"),
        "https://nodejs.org/dist/v24.11.1/node-v24.11.1-linux-x64.tar.gz"
    );
    assert_eq!(
        linux.sums_url("v24.11.1"),
        "https://nodejs.org/dist/v24.11.1/SHASUMS256.txt"
    );
    assert_eq!(linux.folder("v24.11.1"), "node-v24.11.1-linux-x64");
    let alpine = node_dist("linux", "aarch64", true).expect("a build");
    assert_eq!(
        alpine.url("v24.11.1"),
        "https://unofficial-builds.nodejs.org/download/release/v24.11.1/node-v24.11.1-linux-arm64-musl.tar.gz"
    );
    let mac = node_dist("macos", "aarch64", false).expect("a build");
    assert_eq!(mac.archive("v24.11.1"), "node-v24.11.1-darwin-arm64.tar.gz");
    let windows = node_dist("windows", "x86_64", false).expect("a build");
    assert!(windows.is_zip());
    assert_eq!(windows.archive("v24.11.1"), "node-v24.11.1-win-x64.zip");
    assert_eq!(node_dist("freebsd", "x86_64", false), None);
    assert_eq!(node_dist("linux", "riscv64", false), None);
}

#[test]
fn the_newest_lts_with_this_build_is_picked() {
    let index = r#"[
            {"version":"v27.1.0","lts":false,"files":["linux-x64","win-x64-zip"]},
            {"version":"v26.2.0","lts":"Lithium","files":["linux-x64"]},
            {"version":"v24.11.1","lts":"Krypton","files":["linux-x64","win-x64-zip"]},
            {"version":"v20.19.0","lts":"Iron","files":["linux-x64","win-x64-zip"]}
        ]"#;
    let linux = node_dist("linux", "x86_64", false).expect("a build");
    assert_eq!(linux.newest_lts(index).expect("a release"), "v26.2.0");
    let windows = node_dist("windows", "x86_64", false).expect("a build");
    assert_eq!(windows.newest_lts(index).expect("a release"), "v24.11.1");
    let mac = node_dist("macos", "aarch64", false).expect("a build");
    assert!(mac.newest_lts(index).is_err());
    let old = r#"[{"version":"v20.19.0","lts":"Iron","files":["linux-x64"]}]"#;
    assert!(linux.newest_lts(old).is_err(), "too old for the adapters");
    let odd = r#"[{"version":"v24.1.0/../x","lts":"Krypton","files":["linux-x64"]}]"#;
    assert!(linux.newest_lts(odd).is_err());
}

#[test]
fn musl_is_told_by_its_loader() {
    let alpine = tempfile::tempdir().expect("a temp folder");
    assert!(!is_musl(alpine.path()));
    std::fs::create_dir_all(alpine.path().join("lib")).expect("lib");
    std::fs::write(alpine.path().join("lib/ld-musl-x86_64.so.1"), "").expect("musl");
    assert!(is_musl(alpine.path()));
    // Ubuntu with the musl package: glibc's loader says glibc.
    let ubuntu = tempfile::tempdir().expect("a temp folder");
    for dir in ["lib", "lib64"] {
        std::fs::create_dir_all(ubuntu.path().join(dir)).expect("folders");
    }
    std::fs::write(ubuntu.path().join("lib/ld-musl-x86_64.so.1"), "").expect("musl");
    std::fs::write(ubuntu.path().join("lib64/ld-linux-x86-64.so.2"), "").expect("glibc");
    assert!(!is_musl(ubuntu.path()));
}
