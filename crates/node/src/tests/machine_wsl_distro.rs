use super::*;

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

#[test]
fn the_list_reads_as_utf16_or_utf8() {
    let listed = "Ubuntu\r\nDebian\r\n";
    let mut marked = vec![0xFF, 0xFE];
    marked.extend(utf16(listed));

    for bytes in [utf16(listed), marked, listed.as_bytes().to_vec()] {
        let names: Vec<String> = distros(&decode(&bytes))
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(names, ["Ubuntu", "Debian"]);
    }
}

#[test]
fn app_distributions_and_odd_lines_are_left_out() {
    let listed = "Ubuntu-24.04\ndocker-desktop\ndocker-desktop-data\nrancher-desktop\n\0\n\nno distributions here\n";
    let names: Vec<String> = distros(listed).iter().map(ToString::to_string).collect();
    assert_eq!(names, ["Ubuntu-24.04"]);
}

#[test]
fn a_name_is_what_wsl_registers() {
    assert!("Ubuntu-24.04".parse::<DistroName>().is_ok());
    assert!("my_distro".parse::<DistroName>().is_ok());
    assert_eq!("".parse::<DistroName>(), Err(NameError::Empty));
    for bad in ["two words", "-d", "a;b", "Ubuntu\n"] {
        assert_eq!(
            bad.parse::<DistroName>(),
            Err(NameError::Invalid(bad.to_owned()))
        );
    }
}

#[test]
fn the_probe_finds_the_home_linux_agents_and_adapters() {
    let printed = "\
home /home/sam
agent codex /home/sam/.npm-global/bin/codex
agent claude /home/sam/.local/bin/claude
agent pi /mnt/c/Users/sam/AppData/Roaming/npm/pi
agent claude /usr/bin/claude
tool codex-acp /home/sam/.npm-global/bin/codex-acp
tool npx /mnt/c/Program Files/nodejs/npx
some line a profile printed
";
    assert_eq!(
        found(printed),
        Found {
            home: Some("/home/sam".parse().unwrap()),
            agents: vec![
                FoundAgent {
                    name: "claude".into(),
                    path: "/home/sam/.local/bin/claude".parse().unwrap(),
                },
                FoundAgent {
                    name: "codex".into(),
                    path: "/home/sam/.npm-global/bin/codex".parse().unwrap(),
                },
            ],
            tools: vec![FoundAgent {
                name: "codex-acp".into(),
                path: "/home/sam/.npm-global/bin/codex-acp".parse().unwrap(),
            }],
        }
    );
    assert_eq!(found(""), Found::default());
}

#[test]
fn the_verbose_list_gives_names_and_whether_each_runs() {
    let verbose = "  NAME              STATE           VERSION\r\n\
                   * Ubuntu-24.04    Running         2\r\n\
                   \x20 Debian          Stopped         2\r\n\
                   \x20 docker-desktop  Running         2\r\n";
    let read = listed(verbose).expect("English states");
    let names: Vec<(String, bool, bool)> = read
        .iter()
        .map(|listed| (listed.name.to_string(), listed.running, listed.default))
        .collect();
    assert_eq!(
        names,
        [
            ("Ubuntu-24.04".to_owned(), true, true),
            ("Debian".to_owned(), false, false)
        ]
    );
    assert_eq!(listed(&decode(&utf16(verbose))), Some(read));
}

#[test]
fn a_state_in_another_language_asks_for_the_quiet_lists() {
    let verbose = "  NAME      STATUS            VERSION\n* Ubuntu    Wird ausgefuehrt  2\n";
    assert_eq!(listed(verbose), None);
    assert_eq!(listed(""), Some(Vec::new()));
}

#[test]
fn a_windows_drive_path_is_under_mnt_in_wsl() {
    let cases = [
        (r"C:\Users\sam\code", Some("/mnt/c/Users/sam/code")),
        (r"\\?\D:\work\", Some("/mnt/d/work")),
        ("C:", Some("/mnt/c")),
        (r"\\server\share\code", None),
        (r"C:code", None),
        ("/home/sam", None),
    ];
    for (windows, linux) in cases {
        let mapped = linux_path(Path::new(windows)).map(|path| path.to_string());
        assert_eq!(mapped.as_deref(), linux, "{windows}");
    }
}

#[test]
fn a_linux_path_in_wsl_is_under_the_wsl_share() {
    let distro: DistroName = "Ubuntu".parse().expect("a name");
    let home: LinuxPath = "/home/sam/".parse().unwrap();
    assert_eq!(
        windows_path(&distro, &home.join(".claude")),
        PathBuf::from(r"\\wsl$\Ubuntu\home\sam\.claude")
    );
    assert!("home/sam".parse::<LinuxPath>().is_err());
}

#[test]
fn a_mounted_drive_path_reads_as_windows() {
    let cases = [
        ("/mnt/c/Users/sam", Some(r"C:\Users\sam")),
        ("/mnt/d", Some(r"D:\")),
        ("/mnt/cx/y", None),
        ("/home/sam", None),
    ];
    for (linux, windows) in cases {
        assert_eq!(windows_drive_path(linux).as_deref(), windows, "{linux}");
    }
}

#[test]
fn a_folder_in_wsl_reads_by_its_drive_or_through_the_share() {
    let distro: DistroName = "Ubuntu".parse().expect("a name");
    assert_eq!(
        windows_folder(&distro, "/mnt/c/code"),
        PathBuf::from(r"C:\code")
    );
    assert_eq!(
        windows_folder(&distro, "/home/sam/app"),
        PathBuf::from(r"\\wsl$\Ubuntu\home\sam\app")
    );
}
