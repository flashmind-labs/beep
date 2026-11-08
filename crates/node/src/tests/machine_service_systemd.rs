use super::super::tests::launch;
use super::*;

#[test]
fn a_restart_only_follows_a_unit_that_runs_this_binary() {
    let unit = unit(&launch(None), Scope::User);
    assert!(runs(&unit, Path::new("/opt/beep node/beep-node")));
    assert!(!runs(&unit, Path::new("/opt/beep node/beep")));
    assert!(!runs(&unit, Path::new("/usr/local/bin/beep-node")));
}

#[test]
fn the_unit_passes_the_workspace_and_escapes_it() {
    let mut launch = launch(Some("/home/user/my \"$work\" 100%"));
    launch.dir = PathBuf::from("/srv/a\\b");
    let unit = unit(&launch, Scope::User);
    assert!(
        unit.contains(
            "ExecStart=\"/opt/beep node/beep-node\" \"--dir\" \"/srv/a\\\\b\" \
             \"--workspace\" \"/home/user/my \\\"$$work\\\" 100%%\" \"run\"\n"
        ),
        "{unit}"
    );
}

#[test]
fn an_older_unit_is_rewritten_with_its_command_and_path() {
    let current = unit(&launch(Some("/home/user/work")), Scope::User);
    assert_eq!(refreshed(&current, Scope::User), None);
    // What `service install` wrote before KillMode and the node's own
    // log filter.
    let older = current
        .replace("KillMode=mixed\nTimeoutStopSec=20\n", "")
        .replace("\n\n[Install]", "\nEnvironment=RUST_LOG=info\n\n[Install]");
    assert_ne!(older, current);
    assert_eq!(
        refreshed(&older, Scope::User).as_deref(),
        Some(current.as_str())
    );
}

#[test]
fn the_unit_names_its_folder() {
    let mut spaced = launch(None);
    spaced.dir = PathBuf::from("/srv/my $node/50%");
    assert_eq!(
        dir_in(&unit(&spaced, Scope::System)),
        Some(PathBuf::from("/srv/my $node/50%"))
    );
    let sample = "[Service]\nExecStart=\"/home/u/.local/bin/beep-node\" \"--dir\" \
        \"/home/u/.config/beep-node\" \"run\"\nRestart=always\n";
    assert_eq!(
        dir_in(sample),
        Some(PathBuf::from("/home/u/.config/beep-node"))
    );
    assert_eq!(dir_in("[Service]\nRestart=always\n"), None);
}

#[test]
fn a_system_unit_starts_with_the_machine() {
    let user = unit(&launch(None), Scope::User);
    let system = unit(&launch(None), Scope::System);
    assert!(user.ends_with("WantedBy=default.target\n"), "{user}");
    assert_eq!(user.replace("default.target", "multi-user.target"), system);
}
