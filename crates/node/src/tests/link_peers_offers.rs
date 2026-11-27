use super::*;
use crate::agents::Parts;

#[test]
fn a_machine_not_lent_keeps_its_status_but_gives_no_shell_task_or_screen() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let offers = Offers::local(Parts::default(), Provision::new(dir.path()));
    assert!(offers.lends_to_peers(), "lent until the host says not");
    offers.lend_to_peers(false);
    // Offers made earlier from this one share the setting.
    let copy = offers.clone();
    for protocol in [
        "shell/1",
        "terminal/1",
        "device/1",
        "device-video/1",
        "process/1",
        "desktop/1",
        "sign-in/1",
        mcp_servers::PROTOCOL,
        "agents/1",
        "agent-view/1",
        "events/1",
        TRANSCRIPT_PROTOCOL,
        "claude/1",
        "codex/1",
    ] {
        assert!(
            withheld(protocol, copy.lends_to_peers()).is_some(),
            "{protocol}"
        );
    }
    for protocol in ["status/1", "version/1", "tools/1", "files/1"] {
        assert_eq!(
            withheld(protocol, copy.lends_to_peers()),
            None,
            "{protocol}"
        );
    }

    offers.lend_to_peers(true);
    assert_eq!(withheld("shell/1", copy.lends_to_peers()), None);
}
