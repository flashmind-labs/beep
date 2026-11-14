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
fn the_probe_finds_linux_agents_and_the_node() {
    let printed = "\
agent codex /home/sam/.npm-global/bin/codex
agent claude /home/sam/.local/bin/claude
agent gemini /mnt/c/Users/sam/AppData/Roaming/npm/gemini
agent claude /usr/bin/claude
node beep-node 0.7.4
id 4f1c
some line a profile printed
";
    assert_eq!(
        found(printed),
        Found {
            agents: vec!["claude".into(), "codex".into()],
            node: Some("0.7.4".into()),
            machine_id: Some("4f1c".into()),
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
    let names: Vec<(String, bool)> = read
        .iter()
        .map(|listed| (listed.name.to_string(), listed.running))
        .collect();
    assert_eq!(
        names,
        [("Ubuntu-24.04".to_owned(), true), ("Debian".to_owned(), false)]
    );
    assert_eq!(listed(&decode(&utf16(verbose))), Some(read));
}

#[test]
fn a_state_in_another_language_asks_for_the_quiet_lists() {
    let verbose = "  NAME      STATUS            VERSION\n* Ubuntu    Wird ausgefuehrt  2\n";
    assert_eq!(listed(verbose), None);
    assert_eq!(listed(""), Some(Vec::new()));
}
