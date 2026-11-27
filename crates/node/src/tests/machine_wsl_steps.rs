use super::*;

fn strings(args: &[OsString]) -> Vec<String> {
    args.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn listing_asks_for_states_or_names() {
    assert_eq!(strings(&list_args(List::Verbose)), ["--list", "--verbose"]);
    assert_eq!(strings(&list_args(List::All)), ["--list", "--quiet"]);
    assert_eq!(
        strings(&list_args(List::Running)),
        ["--list", "--quiet", "--running"]
    );
}

#[test]
fn a_script_runs_in_the_distributions_login_shell_at_home() {
    let distro: DistroName = "Ubuntu".parse().expect("a name");
    assert_eq!(
        strings(&in_distro(&distro, "true")),
        ["-d", "Ubuntu", "--cd", "~", "--exec", "sh", "-lc", "true"]
    );
}

#[test]
fn the_probe_looks_for_every_harness() {
    let script = probe_script();
    for harness in HARNESSES {
        assert!(
            script.contains(&format!(" {} ", harness.agent))
                || script.contains(&format!(" {};", harness.agent))
        );
    }
    // The folders the node itself looks in.
    assert!(
        HOME_BINS
            .iter()
            .all(|dir| script.contains(&format!("$HOME/{dir}")))
    );
    assert!(
        NODE_VERSIONS
            .iter()
            .all(|(versions, _)| script.contains(versions))
    );
}

#[test]
fn a_failure_keeps_its_last_lines() {
    assert_eq!(reason("one\n\ntwo\nthree\nfour\n"), "two three four");
    assert_eq!(reason("\n"), "wsl.exe failed and said nothing");
}

#[cfg(unix)]
#[test]
fn the_probe_script_runs_in_sh() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().expect("a home");
    let bin = home.path().join(".local/bin");
    let nvm = home.path().join(".nvm/versions/node/v22.1.0/bin");
    std::fs::create_dir_all(&nvm).expect("nvm");
    std::fs::create_dir_all(&bin).expect("bin");
    let scripts = [("claude", "#!/bin/sh\n")];

    let codex = nvm.join("codex");
    std::fs::write(&codex, "#!/bin/sh\n").expect("written");
    std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o755)).expect("mode");

    for (name, text) in scripts {
        let path = bin.join(name);
        std::fs::write(&path, text).expect("written");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("mode");
    }

    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(probe_script())
        .env_clear()
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("sh runs");
    assert!(out.status.success());
    let found = distro::found(&String::from_utf8_lossy(&out.stdout));
    let names: Vec<&str> = found
        .agents
        .iter()
        .map(|agent| agent.name.as_str())
        .collect();
    assert_eq!(names, ["claude", "codex"]);
}

#[test]
fn wslenv_adds_each_name_once() {
    assert_eq!(
        joined_wslenv(
            Some("USERPROFILE/p:GH_TOKEN"),
            ["GH_TOKEN", "BEEP_MCP_TOKEN"].into_iter()
        ),
        "USERPROFILE/p:GH_TOKEN:BEEP_MCP_TOKEN"
    );
    assert_eq!(joined_wslenv(None, ["A"].into_iter()), "A");
}

#[test]
fn an_agent_runs_on_the_path_the_probe_found_it_on() {
    let distro: DistroName = "Ubuntu".parse().expect("a name");
    let cwd: LinuxPath = "/mnt/c/code".parse().expect("a path");
    let cli: LinuxPath = "/home/sam/.local/bin/claude".parse().expect("a path");
    let command = agent_command(&distro, Some(&cwd), &cli, &["--print".to_owned()]);
    let args: Vec<String> = command
        .as_std()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();

    assert_eq!(
        args[..7],
        ["-d", "Ubuntu", "--cd", "/mnt/c/code", "--exec", "sh", "-lc"]
    );
    assert!(args[7].starts_with(&path_prelude()) && args[7].ends_with("exec \"$0\" \"$@\"\n"));
    assert_eq!(args[8..], ["/home/sam/.local/bin/claude", "--print"]);
}
