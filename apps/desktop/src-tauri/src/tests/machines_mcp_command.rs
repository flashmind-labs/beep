use super::*;

#[test]
fn the_app_starts_on_its_own_arguments() {
    assert_eq!(run(Vec::<OsString>::new()), None);
    assert_eq!(run([OsString::from("--login")]), None);
}

#[test]
fn a_session_command_runs_the_pipe_and_fails_without_a_node() {
    // Without the token, or with it and no node at that folder, the
    // pipe fails rather than the app starting.
    let args = ["--dir", "/srv/node", "mcp", "--session", "a3f9c1"].map(OsString::from);
    assert_eq!(run(args), Some(1));
}

#[test]
fn a_bridge_command_runs_the_bridge_instead_of_the_app() {
    // A relay into a distribution that cannot exist fails rather than the
    // app starting, which would start another node and another bridge.
    let args = ["wsl-acp", "--wsl", "no such distro!", "--", "/bin/agent"].map(OsString::from);
    assert_eq!(run(args), Some(1));
}
