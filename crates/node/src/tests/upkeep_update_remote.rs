use super::*;
use crate::machine::service::Manager;

#[test]
fn a_running_service_restarts_the_node() {
    let restarted = Restart::Restarted(Manager::OpenRc);
    let chosen = next(restarted, PathBuf::from("/bin/beep-node"), true);
    assert!(matches!(chosen, Some(Next::Service(_))), "{chosen:?}");
}

#[test]
fn a_node_with_no_service_of_its_own_restarts_itself_on_unix() {
    let exe = PathBuf::from("/bin/beep-node");
    for restart in [
        Restart::NotInstalled,
        Restart::Stopped(Manager::Systemd),
        Restart::OtherBinary(PathBuf::from("/etc/init.d/beep-node")),
    ] {
        let chosen = next(restart.clone(), exe.clone(), true);
        assert!(
            matches!(&chosen, Some(Next::Itself(path)) if *path == exe),
            "{restart:?}: {chosen:?}"
        );
        assert!(next(restart, exe.clone(), false).is_none());
    }
}
