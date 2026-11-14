use super::*;

fn strings(args: &[OsString]) -> Vec<String> {
    args.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn listing_asks_for_names_only() {
    assert_eq!(strings(&list_args(false)), ["--list", "--quiet"]);
    assert_eq!(
        strings(&list_args(true)),
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
fn the_probe_looks_for_every_harness_and_the_node() {
    let script = probe_script();
    for harness in HARNESSES {
        assert!(script.contains(&format!(" {} ", harness.agent)) || script.contains(&format!(" {};", harness.agent)));
    }
    assert!(script.contains("version") && script.contains(" id "));
}

#[test]
fn the_ticket_never_goes_on_the_command_line() {
    assert!(JOIN.contains("mesh join --no-service -"));
    assert!(RUN.contains("--with-parent run"));
    assert!(install_script().contains(INSTALL_URL));
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
    std::fs::create_dir_all(&bin).expect("bin");
    let scripts = [
        ("claude", "#!/bin/sh\n"),
        (
            "beep-node",
            "#!/bin/sh\ncase $1 in version) echo 'beep-node 0.7.4';; id) echo 4f1c;; esac\n",
        ),
    ];

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
    assert_eq!(found.agents, ["claude"]);
    assert_eq!(found.node.as_deref(), Some("0.7.4"));
    assert_eq!(found.machine_id.as_deref(), Some("4f1c"));
}
