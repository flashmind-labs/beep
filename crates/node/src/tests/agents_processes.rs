use super::*;

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
    let unfollowed = tokio::spawn(async move {
        let started = Instant::now();
        assert!(pace(&mut seen).await);
        (started.elapsed(), seen)
    });
    tokio::time::sleep(WATCHED * 2).await;
    assert!(!unfollowed.is_finished());
    // A follower arriving is answered at once, without waiting out the slow pace.
    let waited_from = Instant::now();
    demand.send_replace(1);
    let (elapsed, mut seen) = unfollowed.await.unwrap();
    assert_eq!(elapsed, WATCHED * 2);
    assert_eq!(Instant::now(), waited_from);

    let started = Instant::now();
    assert!(pace(&mut seen).await);
    assert_eq!(started.elapsed(), WATCHED);

    demand.send_replace(0);
    assert!(pace(&mut seen).await);
    let started = Instant::now();
    assert!(pace(&mut seen).await);
    assert_eq!(started.elapsed(), UNWATCHED);
    drop(demand);
    assert!(!pace(&mut seen).await);
}

#[test]
fn following_processes_holds_demand_until_dropped() {
    let mut seen = DEMAND.subscribe();
    let held = Demand::new();
    assert!(seen.has_changed().unwrap());
    assert!(*seen.borrow_and_update() >= 1);
    drop(held);
    assert!(seen.has_changed().unwrap());
}
