use super::*;
use beep_proto::watched;

#[test]
fn adapters_and_harness_helpers_are_hidden_but_commands_are_not() {
    let process = |command: &str| Process {
        pid: 1,
        parent: 0,
        group: 1,
        birth: 1,
        started_at: 1,
        command: command.into(),
    };
    for command in [
        "node /srv/tools/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js",
        "codex app-server",
        "codex.exe app-server",
        "sh -c codex-acp",
        "beep-node --dir /srv/node mcp --session s1",
        "bwrap --unshare-pid",
        "/usr/bin/node /srv/npx/node_modules/@openai/codex/bin/codex.js app-server",
        "/srv/npx/node_modules/@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl/bin/codex-code-mode-host",
        "/opt/tools/cua_node/bin/node_repl",
        "/opt/tools/cua_node/bin/node /opt/tools/cua_node/lib/node_modules/@oai/cua-repl/bin/cua-repl.mjs",
        "node /opt/tools/node_modules/@oai/cua-repl/bin/cua-repl.mjs",
    ] {
        assert!(process(command).helper(), "{command}");
    }
    for command in [
        "npm run dev",
        "node server.js",
        "tsc --watch",
        "echo codex",
        "sh -c 'echo codex'",
        "pip install x",
        "node /srv/app/codex-report.js",
        "node_repl",
        "node /srv/app/cua-repl.mjs",
        "/opt/tools/cua_node/bin/node /srv/app/server.js",
    ] {
        assert!(!process(command).helper(), "{command}");
    }
}

#[tokio::test(start_paused = true)]
async fn the_process_table_is_read_every_second_only_while_followed() {
    let (demand, mut seen) = tokio::sync::watch::channel(0usize);
    let (_session, mut idle) = tokio::sync::watch::channel(false);
    let unfollowed = tokio::spawn(async move {
        let started = Instant::now();
        // Woken for a follower, the read wants a table no older than a second.
        assert_eq!(
            pace(&mut seen, &mut idle, |works: &bool| *works).await,
            Some(WATCHED)
        );
        (started.elapsed(), seen, idle)
    });
    tokio::time::sleep(WATCHED * 2).await;
    assert!(!unfollowed.is_finished());
    // A follower arriving is answered at once, without waiting out the slow pace.
    let waited_from = Instant::now();
    demand.send_replace(1);
    let (elapsed, mut seen, mut idle) = unfollowed.await.unwrap();
    assert_eq!(elapsed, WATCHED * 2);
    assert_eq!(Instant::now(), waited_from);

    let started = Instant::now();
    assert_eq!(
        pace(&mut seen, &mut idle, |works: &bool| *works).await,
        Some(WATCHED)
    );
    assert_eq!(started.elapsed(), WATCHED);

    demand.send_replace(0);
    let started = Instant::now();
    // Unwatched, the read may share a table as old as the time it waited.
    assert_eq!(
        pace(&mut seen, &mut idle, |works: &bool| *works).await,
        Some(UNWATCHED)
    );
    assert_eq!(started.elapsed(), UNWATCHED);
    drop(demand);
    assert_eq!(
        pace(&mut seen, &mut idle, |works: &bool| *works).await,
        None
    );
}

#[tokio::test(start_paused = true)]
async fn the_process_table_is_read_every_second_while_the_session_works() {
    let (_demand, mut seen) = tokio::sync::watch::channel(0usize);
    let (session, mut works) = tokio::sync::watch::channel(false);
    let idle = tokio::spawn(async move {
        let started = Instant::now();
        assert_eq!(
            pace(&mut seen, &mut works, |works: &bool| *works).await,
            Some(WATCHED)
        );
        (started.elapsed(), seen, works)
    });
    tokio::time::sleep(WATCHED * 2).await;
    assert!(!idle.is_finished());
    // A turn starting is answered at once, so a quick wrapper's child is seen.
    session.send_replace(true);
    let (elapsed, mut seen, mut works) = idle.await.unwrap();
    assert_eq!(elapsed, WATCHED * 2);

    let started = Instant::now();
    assert_eq!(
        pace(&mut seen, &mut works, |works: &bool| *works).await,
        Some(WATCHED)
    );
    assert_eq!(started.elapsed(), WATCHED);

    session.send_replace(false);
    let started = Instant::now();
    assert_eq!(
        pace(&mut seen, &mut works, |works: &bool| *works).await,
        Some(UNWATCHED)
    );
    assert_eq!(started.elapsed(), UNWATCHED);
}

#[test]
fn following_processes_holds_demand_until_dropped() {
    let mut seen = DEMAND.subscribe();
    let held = Demand::new();
    assert!(seen.has_changed().unwrap());
    assert!(watched::read_and_update(&mut seen, |seen| *seen) >= 1);
    drop(held);
    assert!(seen.has_changed().unwrap());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_port_opened_or_closed_after_the_last_list_is_a_change() {
    // The session's root is the agent, left out of the list: this test is its child.
    let processes = Processes::new(std::os::unix::process::parent_id());
    let listed = |port| {
        crate::lock(&processes.known)
            .ports
            .contains(&(std::process::id(), port))
    };
    processes.list().await.unwrap();

    // Other tests in this process may open ports too, so only changes are asserted.
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    assert!(processes.ports_changed().await.unwrap());
    assert!(listed(port));

    drop(listener);
    assert!(processes.ports_changed().await.unwrap());
    assert!(!listed(port));
}
