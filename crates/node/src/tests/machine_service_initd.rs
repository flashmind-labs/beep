use super::super::tests::launch;
use super::*;

#[test]
fn the_scripts_take_the_launch_and_keep_nothing_unfilled() {
    for init in [Init::OpenRc, Init::SysV] {
        let script = script(&launch(Some("/home/user/work")), init);
        assert!(!script.contains('@'), "{script}");
        assert!(
            script.contains("command=\"'/opt/beep node/beep-node'\"\n"),
            "{script}"
        );
        assert!(
            script.contains(
                "command_args=\"'--dir' '/srv/node' '--workspace' '/home/user/work' 'run'\"\n"
            ),
            "{script}"
        );
        let search = match init {
            Init::OpenRc => "supervise_daemon_args=\"--env 'PATH=/usr/bin:/home/user/50%/bin'\"\n",
            Init::SysV => "PATH='/usr/bin:/home/user/50%/bin':/sbin",
        };
        assert!(script.contains(search), "{script}");
        assert!(script.contains(LOG), "{script}");
        assert_eq!(written_for(&script), init);
    }
}

#[test]
fn the_script_names_its_folder_and_binary() {
    let mut odd = launch(None);
    odd.dir = PathBuf::from("/srv/it's a \"$node\" `x` \\ 50%");
    for init in [Init::OpenRc, Init::SysV] {
        let script = script(&odd, init);
        assert_eq!(dir_in(&script), Some(odd.dir.clone()), "{script}");
        assert!(runs(&script, Path::new("/opt/beep node/beep-node")));
        assert!(!runs(&script, Path::new("/opt/beep node/beep")));
    }
    assert_eq!(dir_in("#!/bin/sh\ncommand=x\n"), None);
}

/// The scripts' quoting survives the shell: `eval` of the two values
/// gives back the binary and the arguments.
#[cfg(unix)]
#[test]
fn the_shell_reads_back_the_command_line() {
    let mut odd = launch(Some("/home/user/my \"$work\" it's"));
    odd.dir = PathBuf::from("/srv/a\\b `c`");
    let script = script(&odd, Init::SysV);
    let mut values: String = script
        .lines()
        .filter(|line| line.starts_with("command"))
        .map(|line| format!("{line}\n"))
        .collect();
    values.push_str("eval \"set -- $command $command_args\"\nprintf '%s\\n' \"$@\"\n");
    let out = Command::new("sh")
        .args(["-c", &values])
        .output()
        .expect("sh runs");
    let mut expected = vec![odd.exe.display().to_string()];
    expected.extend(odd.args());
    let printed = String::from_utf8_lossy(&out.stdout);
    assert_eq!(printed.lines().collect::<Vec<_>>(), expected, "{script}");
}
